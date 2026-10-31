use std::{cell::Cell, sync::atomic::{AtomicU64, AtomicUsize, Ordering}};
macro_rules! check {
    ($ty:ty, $max:expr, $label:expr) => {{
        let value=<$ty>::new(7);
        assert_eq!(value.try_update(Ordering::AcqRel,Ordering::Acquire,|n|n.checked_add(1)),Ok(7));
        assert_eq!(value.load(Ordering::Acquire),8);
        assert_eq!(value.try_update(Ordering::AcqRel,Ordering::Acquire,|_|None),Err(8));
        assert_eq!(value.load(Ordering::Acquire),8);
        value.store($max,Ordering::Release);
        assert_eq!(value.try_update(Ordering::AcqRel,Ordering::Acquire,|n|n.checked_add(1)),Err($max));
        assert_eq!(value.load(Ordering::Acquire),$max);
        value.store(0,Ordering::Release);
        let calls=Cell::new(0usize);
        assert_eq!(value.try_update(Ordering::AcqRel,Ordering::Acquire,|n|{
            let call=calls.get();calls.set(call+1);
            if call==0 { value.store(1,Ordering::Release); }
            n.checked_add(1)
        }),Ok(1));
        assert!(calls.get()>=2);
        assert_eq!(value.load(Ordering::Acquire),2);
        println!("TRY_UPDATE_CASE_PASS atomic={} success=true refusal_unchanged=true overflow_no_wrap=true stale_cas_retried=true",$label);
    }};
}
fn main(){
    check!(AtomicU64,u64::MAX,"u64");
    check!(AtomicUsize,usize::MAX,"usize");
    let generation=AtomicU64::new(usize::MAX as u64);
    assert_eq!(generation.try_update(Ordering::AcqRel,Ordering::Acquire,|n|n.checked_add(1).filter(|v|*v<=usize::MAX as u64)),Err(usize::MAX as u64));
    assert_eq!(generation.load(Ordering::Acquire),usize::MAX as u64);
    println!("TRY_UPDATE_HOST_PASS atomic_types=2 token_issuer_bound_no_wrap=true");
}
