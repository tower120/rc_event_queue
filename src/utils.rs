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