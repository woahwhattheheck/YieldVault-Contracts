# PR82 pause-limit check

This isolated controller runs the existing three pause tests against a bounded correction of source 51392334965673feb3a5a2e590cfa8e5d8449bfb. It can retain a new source-only candidate reference; it cannot advance the original pull-request branch. Final original-branch publication is performed separately after reading the result and checking the current head. No full suite or live-chain call is requested.
