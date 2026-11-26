# Bench-only Superset settings (mounted at /app/pythonpath/). Never use these
# in production: CSRF is off so the seed script can create content over the API
# with a bearer token alone, and the secret key is a throwaway value.
import os

SECRET_KEY = os.environ.get("SUPERSET_SECRET_KEY", "hs-bench-not-a-secret-key-change-me-0123456789")
WTF_CSRF_ENABLED = False
TALISMAN_ENABLED = False
PREVENT_UNSAFE_DB_CONNECTIONS = False
