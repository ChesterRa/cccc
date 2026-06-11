"""Federation kernel package — registration store, receipt/idempotency store
and authorization helpers for the outbound remote-send feature (Stage 1).

This layer is pure storage + decision logic. It does not perform any network
I/O, transport selection, daemon dispatch or web routing.
"""
