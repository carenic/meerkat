Written by meerkat v0.9.3's own `Session` through
`Session::append_realtime_transcript_event` (a user item observed, its final
transcript, an assistant item observed, two spoken assistant transcript deltas,
then the assistant turn completed). All text is synthetic. The document is the
WholeBlob session 0.9.3 serializes, with a non-empty
`metadata.realtime_transcript_state`: a user item and a spoken assistant item,
each with one content segment keyed `"0"`. Meerkat 0.9.4 refused to cross it
into head-canonical authority (`Corrupted`) until the realtime item maps read
those keys. Used by
`sqlite_store::tests::a_0_9_3_voice_session_crosses_with_its_realtime_projection`.
