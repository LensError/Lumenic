use core::mem::{align_of, size_of};
use core::ptr::{copy_nonoverlapping, NonNull};
use core::slice;

/// Pre-allocated linear per-frame bump arena.
///
/// Resets in O(1) at the end of every frame without heap reallocation.
pub struct FrameArena {
    buf: Vec<u8>,
    offset: usize,
    high_water_mark: usize,
}

impl FrameArena {
    /// Creates a new arena with a fixed byte capacity allocated upfront.
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            offset: 0,
            high_water_mark: 0,
        }
    }

    /// Resets the bump cursor to zero while preserving capacity and `high_water_mark`.
    pub fn reset(&mut self) {
        self.offset = 0;
    }

    /// Returns the number of bytes currently allocated in this frame.
    pub fn used(&self) -> usize {
        self.offset
    }

    /// Returns the total fixed byte capacity of the arena.
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Returns the peak byte offset observed across all frames since creation.
    pub fn high_water_mark(&self) -> usize {
        self.high_water_mark
    }

    /// Returns the backing buffer address for allocation stability checks.
    pub fn buffer_addr(&self) -> usize {
        self.buf.as_ptr() as usize
    }

    /// Allocates `len` bytes aligned to `align` (which must be a power of two).
    pub fn alloc_bytes(&mut self, len: usize, align: usize) -> Option<&mut [u8]> {
        if !align.is_power_of_two() {
            return None;
        }
        let base_addr = self.buf.as_ptr() as usize;
        let cur_addr = base_addr.checked_add(self.offset)?;
        let mask = align - 1;
        let aligned_addr = cur_addr.checked_add(mask)? & !mask;
        let aligned_offset = aligned_addr.checked_sub(base_addr)?;
        let end = aligned_offset.checked_add(len)?;
        if end > self.buf.len() {
            return None;
        }
        self.offset = end;
        if end > self.high_water_mark {
            self.high_water_mark = end;
        }
        Some(&mut self.buf[aligned_offset..end])
    }

    /// Copies a UTF-8 string slice into the frame arena.
    pub fn alloc_str(&mut self, s: &str) -> Option<&str> {
        let dst = self.alloc_bytes(s.len(), 1)?;
        dst.copy_from_slice(s.as_bytes());
        core::str::from_utf8(dst).ok()
    }

    /// Copies a slice of `Copy` elements into an aligned region of the frame arena.
    pub fn alloc_slice_copy<T: Copy>(&mut self, src: &[T]) -> Option<&mut [T]> {
        if src.is_empty() {
            return Some(&mut []);
        }
        if size_of::<T>() == 0 {
            let ptr = NonNull::<T>::dangling().as_ptr();
            // SAFETY: T is zero-sized, so any non-null aligned pointer is valid for src.len() reads/writes
            // without touching memory, and src.len() <= isize::MAX holds for any valid slice.
            return Some(unsafe { slice::from_raw_parts_mut(ptr, src.len()) });
        }

        let byte_len = src.len().checked_mul(size_of::<T>())?;
        // Ensure slice byte length fits in isize::MAX as required by slice::from_raw_parts_mut.
        isize::try_from(byte_len).ok()?;

        let bytes = self.alloc_bytes(byte_len, align_of::<T>())?;
        let dst_ptr = bytes.as_mut_ptr().cast::<T>();

        // SAFETY: alloc_bytes aligned the absolute pointer address to align_of::<T>() and returned a unique
        // mutable slice of exactly byte_len == src.len() * size_of::<T>() bytes within self.buf;
        // src and dst cannot overlap because bytes is uniquely borrowed from self.buf.
        unsafe {
            copy_nonoverlapping(src.as_ptr(), dst_ptr, src.len());
            Some(slice::from_raw_parts_mut(dst_ptr, src.len()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FrameArena;

    #[test]
    fn alignment_bounds_and_overflow() {
        let mut arena = FrameArena::new(256);
        assert_eq!(arena.capacity(), 256);
        assert_eq!(arena.used(), 0);

        for align in [1usize, 4, 8, 16] {
            let b = arena.alloc_bytes(3, align).expect("should fit");
            assert_eq!((b.as_ptr() as usize) % align, 0);
        }

        assert!(arena.alloc_bytes(1, 3).is_none());
        assert!(arena.alloc_bytes(usize::MAX, 1).is_none());

        arena.reset();
        assert_eq!(arena.used(), 0);
        assert!(arena.high_water_mark() > 0);
    }

    #[test]
    fn exact_capacity_and_zero_reallocation_across_resets() {
        let mut arena = FrameArena::new(64);
        let addr_before = arena.buffer_addr();

        for i in 0..100 {
            arena.reset();
            let s = arena.alloc_str("frame-str").expect("string fits");
            assert_eq!(s, "frame-str");
            let nums = arena
                .alloc_slice_copy(&[1u32, 2, 3, i as u32])
                .expect("slice fits");
            assert_eq!(nums, &[1, 2, 3, i as u32]);
            assert_eq!(arena.buffer_addr(), addr_before);
        }

        arena.reset();
        let rem = arena.capacity();
        assert!(arena.alloc_bytes(rem, 1).is_some());
        assert!(arena.alloc_bytes(1, 1).is_none());
        assert_eq!(arena.high_water_mark(), 64);
    }
}
