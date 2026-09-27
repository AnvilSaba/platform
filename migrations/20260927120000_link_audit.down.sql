DROP TABLE mcguildlink.audit_outbox;
DROP TRIGGER enqueue_audit_log ON mcguildlink.audit_logs;
DROP FUNCTION mcguildlink.enqueue_audit_log();
DROP TABLE mcguildlink.audit_logs;
