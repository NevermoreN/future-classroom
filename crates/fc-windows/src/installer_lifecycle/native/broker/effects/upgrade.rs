//! Upgrade effects use one bounded old/new namespace union and actual bad bytes.
use super::*;
fn new_absent(manifest: &Manifest) -> Result<()> {
    let mut found = [0u16; 39];
    let mut context = MSIINSTALLCONTEXT_NONE;
    let rc = unsafe {
        MsiEnumProductsExW(
            &HSTRING::from(format!("{{{}}}", manifest.product)),
            None,
            MSIINSTALLCONTEXT_ALL.0 as u32,
            0,
            Some(&mut found),
            Some(&mut context),
            None,
            None,
        )
    };
    if !known_product_absence(rc) {
        return Err(LifecycleError::Unsettled);
    }
    Ok(())
}
fn predecessor(session: &Session) -> Result<&super::super::upgrade::VerifiedUpgradePlan> {
    session
        .verified_upgrade
        .as_ref()
        .filter(|p| p.matches(session))
        .ok_or(LifecycleError::Invalid)
}
fn observed(session: &Session, proof: &mut Proof) -> Result<preimage::Snapshot> {
    let plan = predecessor(session)?;
    new_absent(&session.manifest)?;
    same_registration(&plan.old, &session.resources.sid)?;
    let snapshot = preimage::capture(proof, &root()?, &plan.union, &session.resources.sid)?;
    if !snapshot.new_only_absent(&plan.old, &plan.union) {
        return Err(LifecycleError::Unsettled);
    }
    markers(proof, &plan.old)?;
    proof.check()?;
    Ok(snapshot)
}
pub(super) fn admit(session: &mut Session) -> Result<Admitted> {
    current(session.deadline)?;
    run_absent_at(RUN_KEY)?;
    let mut proof = Proof::new(session.deadline);
    let result = observed(session, &mut proof);
    proof.release()?;
    Ok(Admitted {
        kind: Admission::Upgrade,
        snapshot: Some(result?),
    })
}
pub(super) fn revalidate(session: &mut Session, admission: &Admitted) -> Result<()> {
    let mut proof = Proof::new(session.deadline);
    let actual = observed(session, &mut proof);
    let same = actual.as_ref().is_ok_and(|v| {
        admission
            .snapshot
            .as_ref()
            .is_some_and(|original| original.same_objects(v))
    });
    proof.release()?;
    actual?;
    if !same {
        return Err(LifecycleError::Unsettled);
    }
    Ok(())
}
fn component_product_absent(manifest: &Manifest) -> Result<()> {
    for entry in &manifest.entries {
        let mut path = [0u16; PATH_LIMIT];
        let mut n = path.len() as u32;
        let state = unsafe {
            MsiGetComponentPathExW(
                &HSTRING::from(format!("{{{}}}", manifest.product)),
                &HSTRING::from(format!("{{{}}}", entry.component)),
                None,
                Some(MSIINSTALLCONTEXT_USERUNMANAGED),
                Some(windows::core::PWSTR(path.as_mut_ptr())),
                Some(&mut n),
            )
        };
        if state != INSTALLSTATE_UNKNOWN && state != INSTALLSTATE_ABSENT {
            return Err(LifecycleError::Unsettled);
        }
    }
    Ok(())
}
pub(super) fn reconcile(
    session: &mut Session,
    admission: &Admitted,
    decision: Decision,
) -> Result<Proof> {
    let plan = predecessor(session)?;
    match decision.expected {
        ExpectedState::Present => {
            let proof = present(session)?;
            new_absent(&plan.old)?;
            component_product_absent(&plan.old)?;
            proof.check()?;
            Ok(proof)
        }
        ExpectedState::CapturedPreimage => {
            let mut proof = Proof::new(session.deadline);
            let actual = observed(session, &mut proof)?;
            if !admission
                .snapshot
                .as_ref()
                .is_some_and(|original| original.same_effects(&actual))
            {
                return Err(LifecycleError::Unsettled);
            }
            component_product_absent(&session.manifest)?;
            proof.check()?;
            Ok(proof)
        }
        ExpectedState::Absent => Err(LifecycleError::Invalid),
    }
}
