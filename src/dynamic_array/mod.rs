#[cfg(test)]
mod test;

use std::mem;
use std::alloc::Layout;
use std::mem::ManuallyDrop;
use std::borrow::Borrow;
use std::marker::PhantomData;

#[repr(C)]
pub struct DynamicArray<Header, T>{
    header: Header,
    array_len : usize,
    array: [T; 0],
}

impl<Header, T> DynamicArray<Header, T>{
    #[inline]
    fn layout(len: usize) -> Layout{
        unsafe{
            let size = mem::size_of::<Self>() + len * mem::size_of::<T>();
            Layout::from_size_align_unchecked(size, mem::align_of::<Self>())
        }
    }

    pub fn construct(header: Header, value: T, len: usize) -> *mut Self {
        unsafe{
            let this = &mut *Self::construct_uninit(header, len);

            for item in this.slice_mut(){
                std::ptr::copy_nonoverlapping(value.borrow(), item, 1);
            }

            std::mem::forget(value);

            this
        }
    }

    /// array is not initialized
    pub unsafe fn construct_uninit(header: Header, len: usize) -> *mut Self {
        let this = &mut *{
            let allocation = std::alloc::alloc(Self::layout(len));
            allocation as *mut Self
        };

        std::ptr::write(&mut this.header, header);
        this.array_len = len;

        this
    }

    /// No checks at all!
    #[inline]
    pub unsafe fn write_at(&mut self, index: usize, value: T){
        std::ptr::write(self.slice_mut().get_unchecked_mut(index), value);
    }

    /// Unsafe due to potential double-free, use-after-free
    pub unsafe fn destruct(this: *mut Self) {
        if std::mem::needs_drop::<T>() {
            for item in (*this).slice_mut(){
                std::ptr::drop_in_place(item);
            }
        }

        Self::destruct_uninit(this);
    }

    pub unsafe fn destruct_uninit(this: *mut Self) {
        if std::mem::needs_drop::<Header>() {
            std::ptr::drop_in_place(&mut (*this).header);
        }

        std::alloc::dealloc(this as *mut u8,Self::layout((*this).array_len));
    }

    #[inline]
    pub fn header(&self) -> &Header{
        &self.header
    }

    #[inline]
    pub fn header_mut(&mut self) -> &mut Header{
        &mut self.header
    }

    #[inline]
    pub fn slice(&self) -> &[T] {
        unsafe {
            std::slice::from_raw_parts(self.array.as_ptr(), self.array_len)
        }
    }

    #[inline]
    pub fn slice_mut(&mut self) -> &mut [T] {
        unsafe {
            std::slice::from_raw_parts_mut(self.array.as_mut_ptr(), self.array_len)
        }
    }

    #[inline]
    pub fn len(&self) -> usize{
        self.array_len
    }
}