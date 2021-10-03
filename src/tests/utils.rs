use crate::event_reader::LendingIterator;

pub fn consume_copies<T: Clone>(iter: &mut impl LendingIterator<ItemValue = T>) -> Vec<T> {
    consume_mapped(iter, |item| item.clone())
}

pub fn consume_mapped<T, F, R>(iter: &mut impl LendingIterator<ItemValue = T>, f: F) -> Vec<R>
where F: Fn(&T) -> R
{
    let mut v = Vec::new();
    while let Some(item) = iter.next(){
        v.push( f(item) );
    }
    v
}

pub fn skip<T>(iter: &mut impl LendingIterator<ItemValue = T>, len : usize) {
    let mut i = 0;
    while let Some(_) = iter.next(){
        i+=1;

        if i == len {
            break;
        }
    }
}
