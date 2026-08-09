use std::collections::HashMap;
use std::hash::Hash;

pub(super) const MAX_ACTIVE_STREAMS: usize = 1_024;

/// Bound abandoned stream handles. Evicted streams safely fall back to their
/// final `chat.message`, so cleanup must prefer eviction over unbounded growth.
pub(super) fn trim_active<K, V>(streams: &mut HashMap<K, V>)
where
    K: Clone + Eq + Hash,
{
    while streams.len() > MAX_ACTIVE_STREAMS {
        let Some(key) = streams.keys().next().cloned() else {
            break;
        };
        streams.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abandoned_streams_are_bounded() {
        let mut streams = (0..=MAX_ACTIVE_STREAMS)
            .map(|index| (index, index))
            .collect();

        trim_active(&mut streams);

        assert_eq!(streams.len(), MAX_ACTIVE_STREAMS);
    }
}
