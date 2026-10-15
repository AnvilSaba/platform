-- 書き込み停止後に旧 DB と移行先で実行し、出力を比較する。資格情報は出力しない。
\pset format unaligned
\pset tuples_only on
SELECT jsonb_build_object('migrations', jsonb_agg(jsonb_build_object(
    'version', version, 'success', success, 'checksum', encode(checksum, 'hex')
) ORDER BY version)) FROM public._sqlx_migrations;

SELECT jsonb_build_object('database', datname, 'owner', pg_get_userbyid(datdba), 'acl', datacl::text)
FROM pg_database WHERE datname = current_database();

SELECT format('SELECT jsonb_build_object(''table'', %L, ''rows'', count(*)) FROM %I.%I;',
    schemaname || '.' || tablename, schemaname, tablename)
FROM pg_tables WHERE schemaname IN ('mcguildlink', 'public') ORDER BY schemaname, tablename
\gexec

SELECT format('SELECT jsonb_build_object(''sequence'', %L, ''last_value'', last_value, ''is_called'', is_called) FROM %I.%I;',
    schemaname || '.' || sequencename, schemaname, sequencename)
FROM pg_sequences WHERE schemaname = 'mcguildlink' ORDER BY sequencename
\gexec

SELECT jsonb_build_object('role', rolname, 'login', rolcanlogin, 'inherit', rolinherit,
    'createdb', rolcreatedb, 'createrole', rolcreaterole, 'replication', rolreplication, 'bypassrls', rolbypassrls)
FROM pg_roles WHERE rolname LIKE 'platform\_%' ESCAPE '\' ORDER BY rolname;

SELECT jsonb_build_object('member', member.rolname, 'role', parent.rolname, 'admin', membership.admin_option,
    'inherit', membership.inherit_option, 'set', membership.set_option)
FROM pg_auth_members membership
JOIN pg_roles member ON member.oid = membership.member
JOIN pg_roles parent ON parent.oid = membership.roleid
WHERE member.rolname LIKE 'platform\_%' ESCAPE '\' ORDER BY member.rolname, parent.rolname;

SELECT jsonb_build_object('schema', nspname, 'owner', pg_get_userbyid(nspowner), 'acl', nspacl::text)
FROM pg_namespace WHERE nspname IN ('mcguildlink', 'public') ORDER BY nspname;

SELECT jsonb_build_object('object', namespace.nspname || '.' || relation.relname,
    'owner', pg_get_userbyid(relation.relowner), 'acl', relation.relacl::text)
FROM pg_class relation JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
WHERE namespace.nspname IN ('mcguildlink', 'public') AND relation.relkind IN ('r', 'S')
ORDER BY namespace.nspname, relation.relname;

SELECT jsonb_build_object('table', table_schema || '.' || table_name, 'column', column_name,
    'grantee', grantee, 'privilege', privilege_type, 'grantable', is_grantable)
FROM information_schema.column_privileges WHERE table_schema IN ('mcguildlink', 'public')
ORDER BY table_schema, table_name, column_name, grantee, privilege_type;

SELECT jsonb_build_object('function', namespace.nspname || '.' || routine.proname,
    'arguments', pg_get_function_identity_arguments(routine.oid),
    'owner', pg_get_userbyid(routine.proowner), 'security_definer', routine.prosecdef, 'acl', routine.proacl::text)
FROM pg_proc routine JOIN pg_namespace namespace ON namespace.oid = routine.pronamespace
WHERE namespace.nspname = 'mcguildlink' ORDER BY routine.proname, pg_get_function_identity_arguments(routine.oid);

DO $$ BEGIN
    IF EXISTS (SELECT FROM public._sqlx_migrations WHERE NOT success) THEN
        RAISE EXCEPTION 'failed migration in history';
    END IF;
    IF EXISTS (SELECT FROM pg_roles
        WHERE rolname IN ('platform_bot', 'platform_mc_link_server', 'platform_public_api', 'platform_db_migrator')
        AND (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)) THEN
        RAISE EXCEPTION 'runtime or migrator role has elevated privileges';
    END IF;
END $$;
