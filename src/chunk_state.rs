use crate::sync::AtomicU64;
use crate::sync::Ordering;
use crate::{StartPositionEpoch, utils};

pub struct AtomicPackedChunkState(AtomicU64);

impl AtomicPackedChunkState{
    #[inline(always)]
    pub fn new(packed_chunk_state: PackedChunkState) -> Self{
        Self{0: AtomicU64::new(packed_chunk_state.into())}
    }
    #[inline(always)]
    pub fn load(&self, ordering : Ordering) -> PackedChunkState {
         PackedChunkState{0: self.0.load(ordering)}
    }
    #[inline(always)]
    pub fn store(&self, packed_chunk_state: PackedChunkState, ordering : Ordering){
         self.0.store(packed_chunk_state.0, ordering);
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct PackedChunkState(u64);
impl PackedChunkState{
    #[inline(always)]
    pub fn pack(chunk_state: ChunkState) -> Self{
        Self(
            u64::from(chunk_state.len)
            | u64::from(chunk_state.has_next) << 32
            | u64::from(chunk_state.epoch.into_inner()) << 33
        )
    }

    #[inline(always)]
    pub fn unpack(&self) -> ChunkState{
        let len = self.len();
        let has_next = self.has_next();
        let epoch = self.epoch();

        ChunkState{len, has_next, epoch}
    }

    #[inline(always)]
    pub fn len(&self) -> u32 {
        self.0 as u32
    }
    #[inline(always)]
    pub fn set_len(&mut self, new_len: u32) {
        const MASK: u64 = (1 << 32) - 1;    // all first 32 bits 0.
        self.0 &= !MASK;
        self.0 |= new_len as u64;
    }

    #[inline(always)]
    pub fn has_next(&self) -> bool {
        utils::bittest_u64::<32>(self.0)
    }
    #[inline(always)]
    pub fn set_has_next(&mut self, new_flag: bool){
        self.0 = utils::bitset_u64::<32>(self.0, new_flag);
    }

    #[inline(always)]
    pub fn epoch(&self) -> StartPositionEpoch {
        unsafe{ StartPositionEpoch::new_unchecked((self.0 >> 33) as u32) }
    }
    #[inline(always)]
    pub fn set_epoch(&mut self, epoch: StartPositionEpoch){
        const MASK: u64 = (1 << 33) - 1;    // all first 33 bits 0.
        self.0 &= MASK;
        self.0 |= u64::from(epoch.into_inner()) << 33;
    }
}

impl From<PackedChunkState> for u64{
    #[inline(always)]
    fn from(packed_chunk_state: PackedChunkState) -> u64 {
        packed_chunk_state.0
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct ChunkState{
    pub len   : u32,
    pub epoch : StartPositionEpoch,
    pub has_next: bool
}

#[cfg(test)]
mod test{
    use rand::Rng;
    use crate::chunk_state::{ChunkState, PackedChunkState};
    use crate::StartPositionEpoch;

    #[test]
    fn pack_unpack_fuzzy_test(){
        let mut rng = rand::thread_rng();

        for _ in 0..100000{
            let state = ChunkState{
                len: rng.gen_range(0 .. u32::MAX),
                epoch: StartPositionEpoch::new(rng.gen_range(0 .. u32::MAX/2)),
                has_next: rng.gen_bool(0.5)
            };

            let pack = PackedChunkState::pack(state.clone());
            let unpacked = pack.unpack();
            assert_eq!(state, unpacked);
        }
    }

    #[test]
    fn pack_unpack_data_test(){
        let state = ChunkState{
            len: 1356995898,
            epoch: StartPositionEpoch::new(1624221158),
            has_next: true
        };

        let pack = PackedChunkState::pack(state.clone());
        let unpacked = pack.unpack();
        assert_eq!(state, unpacked);
    }

    #[test]
    fn setters_fuzzy_test(){
        let mut rng = rand::thread_rng();

        for _ in 0..100000{
            let state1 = ChunkState{
                len: rng.gen_range(0 .. u32::MAX),
                epoch: StartPositionEpoch::new(rng.gen_range(0 .. u32::MAX/2)),
                has_next: rng.gen_bool(0.5)
            };
            let state2 = ChunkState{
                len: rng.gen_range(0 .. u32::MAX),
                epoch: StartPositionEpoch::new(rng.gen_range(0 .. u32::MAX/2)),
                has_next: rng.gen_bool(0.5)
            };

            let mut pack1 = PackedChunkState::pack(state1.clone());
            pack1.set_len(state2.len);
            pack1.set_has_next(state2.has_next);
            pack1.set_epoch(state2.epoch);

            let pack2 = PackedChunkState::pack(state2.clone());
            assert_eq!(pack1, pack2);

            let unpacked1 = pack1.unpack();
            let unpacked2 = pack2.unpack();
            assert_eq!(unpacked1, unpacked2);
        }
    }
}