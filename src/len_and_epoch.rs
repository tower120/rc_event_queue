use crate::utils::U32Pair;

pub struct LenAndEpoch{
    pair: U32Pair
}

impl LenAndEpoch{
    #[inline(always)]
    pub fn new(len: u32, epoch: u32) -> Self{
        Self{pair: U32Pair::from_u32(len, epoch)}
    }

    #[inline(always)]
    pub fn len(&self) -> u32 {
        self.pair.first()
    }

    #[inline(always)]
    pub fn epoch(&self) -> u32 {
        self.pair.second()
    }
}

impl From<u64> for LenAndEpoch{
    #[inline(always)]
    fn from(value: u64) -> LenAndEpoch {
        LenAndEpoch{
            pair: U32Pair::from_u64(value)
        }
    }
}

impl From<LenAndEpoch> for u64{
    #[inline(always)]
    fn from(len_and_epoch: LenAndEpoch) -> u64 {
        len_and_epoch.pair.as_u64()
    }
}