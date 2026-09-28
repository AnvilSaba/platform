CREATE OR REPLACE FUNCTION mcguildlink.bump_whitelist_revision() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, mcguildlink AS $$
BEGIN
    UPDATE mcguildlink.whitelist_revision SET revision = revision + 1 WHERE singleton;
    RETURN NULL;
END $$;

ALTER TABLE mcguildlink.whitelist_revision
    DROP COLUMN if_modified_since_safe,
    DROP COLUMN last_modified_at;
