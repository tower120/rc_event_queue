use crate::event_queue::Settings;
use crate::event_reader::Iter;

pub fn consume_copies<T: Clone, S: Settings>(iter: &mut Iter<T, S>) -> Vec<T> {
    consume_mapped(iter, |item| item.clone())
}

pub fn consume_mapped<T, S, F, R>(iter: &mut Iter<T, S>, f: F) -> Vec<R>
where S: Settings,
      F: Fn(&T) -> R
{
    let mut v = Vec::new();
    while let Some(item) = iter.next(){
        v.push( f(item) );
    }
    v
}


pub fn skip<T, S:Settings>(iter: &mut Iter<T, S>, len : usize) {
    let mut i = 0;
    while let Some(_) = iter.next(){
        i+=1;

        if i == len {
            break;
        }
    }
}
