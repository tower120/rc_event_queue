use crate::dynamic_array::DynamicArray;
use itertools::assert_equal;
use std::cell::RefCell;

struct Header<F: FnMut(u8)>{
    i : u8,
    on_destroy : F
}
impl<F: FnMut(u8)> Drop for Header<F>{
    fn drop(&mut self) {
        (self.on_destroy)(self.i);
    }
}

struct Data<F: FnMut(usize)>{
    i : usize,
    on_destroy : F
}

impl<F: FnMut(usize)> Drop for Data<F>{
    fn drop(&mut self) {
        (self.on_destroy)(self.i);
    }
}


#[test]
fn uninit_test(){
    let data_destruct_order = RefCell::new(Vec::new());
    let on_destroy = |i:usize|{
        data_destruct_order.borrow_mut().push(i);
    };

    let header_destruct_counter = RefCell::new(0);
    let on_header_destroy = |_|{
        *header_destruct_counter.borrow_mut() += 1;
    };

    let fla = unsafe {
        &mut *DynamicArray::<Header<_>, Data<_>>::construct_uninit(
            Header { i: 100, on_destroy: on_header_destroy },
            8
        )
    };

    unsafe { fla.write_at(1, Data { i: 800, on_destroy }); }

    let array = fla.slice_mut();
    assert_eq!(array.len(), 8);
    assert_eq!(array[1].i, 800);
    assert_eq!(fla.header.i, 100);

    assert_eq!(*header_destruct_counter.borrow(), 0);
    assert!(data_destruct_order.borrow().is_empty());
    unsafe{ DynamicArray::destruct_uninit(fla); }
    assert!(data_destruct_order.borrow().is_empty());
    assert_eq!(*header_destruct_counter.borrow(), 1);
}

#[test]
fn default_test(){
    let data_destruct_order = RefCell::new(Vec::new());
    let on_destroy = |i:usize|{
        data_destruct_order.borrow_mut().push(i);
    };

    let header_destruct_counter = RefCell::new(0);
    let on_header_destroy = |_|{
        *header_destruct_counter.borrow_mut() += 1;
    };


    let fla = unsafe{
        &mut *DynamicArray::<Header<_>, Data<_>>::construct(
                Header { i: 100, on_destroy: on_header_destroy },
                Data { i: 0, on_destroy },
                4
            )
    };

    assert!(data_destruct_order.borrow().is_empty());
    {
        let array = fla.slice_mut();
        array[1] = Data{i : 800, on_destroy};
    }
    assert_equal(&*data_destruct_order.borrow(), &[0 as usize]);
    data_destruct_order.borrow_mut().clear();

    assert_eq!(fla.header.i, 100);
    assert_equal(fla.slice().iter().map(|data|data.i), [0, 800, 0, 0]);

    assert_eq!(*header_destruct_counter.borrow(), 0);
    unsafe{ DynamicArray::destruct(fla); }
    assert_equal(&*data_destruct_order.borrow(), &[0,800,0,0 as usize]);
    assert_eq!(*header_destruct_counter.borrow(), 1);
}

/*#[repr(transparent)]
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
}*/