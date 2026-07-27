# Add a capability

1. Choose a functional name such as `speech.transcribe`; do not name hardware.
2. Extend or introduce a typed contract for request-relevant features.
3. Publish implementations separately from hardware inventory.
4. Require an explicit exposure policy and current availability.
5. Add hard matcher checks with stable rejection codes.
6. Rank only after contract, policy, consent, and availability pass.
7. Record observations separately; never silently rewrite declarations.
8. Test forbidden policy, saturation, missing features, and stable ordering.

Before future execution, the selected node must revalidate its sovereign policy
and consent locally. A coordinator's earlier match is not authorization.

