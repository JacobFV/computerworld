# Atlas rollback plan

Owner: Carol Nakamura. Use this if the release has to come back out.

1. Announce the rollback in #general before touching anything.
2. Stop the rollout: pause the deploy job for release ATLAS-2026.
3. Revert the release tag on the onboarding repository and push the revert.
4. Redeploy the previous tag to app-server and confirm the intranet answers.
5. Reopen OPS-1 with what failed, and link this plan from the ticket.

Rollback is done when the status page shows the previous version and
nobody has an open question in the thread.
