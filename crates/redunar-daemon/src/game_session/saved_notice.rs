//! Deliver successful-save edges once, retrying only an undelivered edge.
pub(super) fn publish_completed<E>(
    completed: u64,
    noticed: &mut u64,
    publish: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    if completed <= *noticed {
        return Ok(());
    }
    publish()?;
    *noticed = completed;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_failures_and_old_completions_do_not_repeat_success() {
        let mut noticed = 4;
        for completed in [0, 3, 4, 4] {
            publish_completed::<()>(completed, &mut noticed, || panic!("no new completion"))
                .unwrap();
        }
        let mut deliveries = 0;
        for completed in [5, 5, 6, 6] {
            publish_completed::<()>(completed, &mut noticed, || {
                deliveries += 1;
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(deliveries, 2);
        assert_eq!(noticed, 6);
    }

    #[test]
    fn unavailable_overlay_is_retried_without_losing_the_completion() {
        let mut noticed = 0;
        assert!(publish_completed(1, &mut noticed, || Err("unavailable")).is_err());
        assert_eq!(noticed, 0);
        publish_completed::<()>(1, &mut noticed, || Ok(())).unwrap();
        assert_eq!(noticed, 1);
        publish_completed::<()>(1, &mut noticed, || panic!("already delivered")).unwrap();
    }
}
