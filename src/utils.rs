use std::ops::{Add};

pub struct U32Pair (u64);
impl U32Pair{
    #[inline(always)]
    pub fn from_u32(first: u32, second: u32) -> Self{
        Self(u64::from(second) << 32 | u64::from(first))
    }

    #[inline(always)]
    pub fn from_u64(value : u64) -> Self{
        Self(value)
    }

    #[inline(always)]
    pub fn as_u64(&self) -> u64{
        self.0
    }

    #[inline(always)]
    pub fn first(&self) -> u32{
        self.0 as u32
    }

    #[inline(always)]
    pub fn second(&self) -> u32{
        (self.0 >> 32) as u32
    }
}

#[inline(always)]
pub fn bittest_u64<const N: u8>(value: u64) -> bool {
    #[allow(unreachable_code)]
    unsafe {
        #[cfg(target_arch = "x86_64")]
        return core::arch::x86_64::_bittest64(&(value as i64), N as i64) != 0;

        return value & (1 << N) != 0;
    }
}
#[inline(always)]
#[must_use]
pub fn bitset_u64<const N: u8>(mut value: u64, bit: bool) -> u64 {
    // should be const. Lets hope rust precalculate it.
    let mask: u64 = 1<<N; // all bits = 0, Nth = 1

    // clear bit
    value &= !mask;

    // set bit
    value |= (bit as u64) << N;

    value
}


#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Epoch<T, const MAX: u64> (T)
    where T : Copy + Add<Output = T> + PartialOrd + From<u8> + Into<u64>;

impl<T, const MAX: u64> Epoch<T, MAX>
    where T : Copy + Add<Output = T> + PartialOrd + From<u8> + Into<u64>
{
    #[inline(always)]
    pub fn zero() -> Self {
        Self{0: T::from(0)}
    }

    pub fn new(init: T) -> Self {
        assert!(init.into() <= MAX);
        Self{0: init}
    }

    #[inline(always)]
    pub unsafe fn new_unchecked(init: T) -> Self {
        Self{0: init}
    }

    /// +1
    #[must_use]
    #[inline]
    pub fn increment(&self) -> Self{
        if self.0.into() == MAX{
            Self::zero()
        } else {
            Self{0: self.0 + T::from(1)}
        }
    }

    #[inline(always)]
    pub fn into_inner(self) -> T{
        self.0
    }
}