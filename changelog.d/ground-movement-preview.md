Ordinary dry-ground movement shows its collision-checked first step as soon as
the local command queue accepts it, without waiting for the worker's reply.
The worker still owns the observed position and movement checks. Its reply
settles that preview once; refusal, correction or withdrawal restores the
observed pose. Sampling cadence, mover-supported movement and water physics
are unchanged. Release and rapid restart can still wait on the sample window.
