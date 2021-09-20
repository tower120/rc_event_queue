use crate::dynamic_array::DynamicArray;
use itertools::assert_equal;
use std::borrow::Borrow;

struct Header{
    i : u8
}
impl Drop for Header{
    fn drop(&mut self) {
        println!("Drop Header");
    }
}

struct Data{
    i : usize
}
impl Drop for Data{
    fn drop(&mut self) {
        println!("Drop Data {:}", self.i);
    }
}


#[test]
fn uninit_test(){
    let mut fla = unsafe {
        &mut *DynamicArray::<Header, Data>::construct_uninit(
            Header { i: 100 },
            8
        )
    };

    unsafe { fla.write_at(1, Data { i: 800 }); }

    let mut array = fla.slice_mut();
    assert_eq!(array.len(), 8);
    assert_eq!(array[1].i, 800);

    unsafe{ DynamicArray::destruct_uninit(fla); }
}

#[test]
fn default_test(){
    let mut fla = unsafe{
        &mut *DynamicArray::<Header, Data>::construct(
                Header { i: 100 },
                Data { i: 0 },
                4
            )
    };

    {
        let array = fla.slice_mut();
        array[1] = Data{i : 800};
    }

    assert_equal(fla.slice().iter().map(|data|data.i), [0, 800, 0, 0]);
    unsafe{ DynamicArray::destruct(fla); }
}

#[repr(transparent)]
struct Node(DynamicArray<Header, usize>);

impl Node{
    pub fn construct()-> &'static mut Self {
        let base = DynamicArray::<Header, usize>::construct(
            Header { i: 20 },
            0,
            10
        );
        unsafe { &mut *(base as *mut _ as *mut Self) }
    }

    pub unsafe fn destruct(this: *mut Self){
        DynamicArray::<Header, usize>::destruct(
            this as *mut DynamicArray<Header, usize>
        );
    }
}