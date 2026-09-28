ALTER TABLE mcguildlink.whitelist_revision
    ADD COLUMN last_modified_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    ADD COLUMN if_modified_since_safe boolean NOT NULL DEFAULT true;

CREATE OR REPLACE FUNCTION mcguildlink.bump_whitelist_revision() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, mcguildlink AS $$
DECLARE
    previous_at timestamptz;
    next_at timestamptz;
BEGIN
    -- 更新番号の行ロックにより、同時トランザクションでも日時の順序を保つ。
    SELECT last_modified_at INTO previous_at
      FROM mcguildlink.whitelist_revision WHERE singleton FOR UPDATE;
    next_at := GREATEST(clock_timestamp(), previous_at);
    UPDATE mcguildlink.whitelist_revision
       SET revision = revision + 1,
           last_modified_at = next_at,
           -- HTTP-date は秒精度。同じ秒の変更には If-Modified-Since を使わない。
           if_modified_since_safe = date_trunc('second', next_at) > date_trunc('second', previous_at)
     WHERE singleton;
    RETURN NULL;
END $$;
