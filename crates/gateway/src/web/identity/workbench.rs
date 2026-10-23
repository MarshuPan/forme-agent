//! Durable Web workbench session metadata, separate from brain state.
use rusqlite::OptionalExtension;

use super::{
    now, storage, storage_error, AuthError, AuthResult, AuthenticatedWebOwner, WebIdentityStore,
};
use forme_protocol as p;

pub(crate) struct WorkbenchRunBinding {
    pub(crate) workspace: p::WorkspaceRef,
    pub(crate) run: Option<p::RunId>,
}

impl WebIdentityStore {
    pub(crate) fn bind_workbench_run(
        &self,
        owner: &AuthenticatedWebOwner,
        request: &p::RunRequest,
        default_workspace: &p::WorkspaceRef,
    ) -> AuthResult<WorkbenchRunBinding> {
        let command = request
            .idempotency_key
            .as_ref()
            .ok_or(AuthError::InvalidInput)?;
        if request.source != p::Source::UserTurn
            || request.session.0.trim().is_empty()
            || command.0.trim().is_empty()
            || command.0.len() > 256
            || command.0.chars().any(char::is_control)
            || default_workspace.0.trim().is_empty()
        {
            return Err(AuthError::InvalidInput);
        }
        let request_digest =
            p::canonical_digest(&(p::SchemaVersion(1), "forme-web-workbench-run-v1", request))
                .map_err(|_| AuthError::Unavailable)?;
        let mut db = self.database()?;
        storage::check_owner(&db.connection, owner, now())?;
        let owner_digest = p::sha256_content_digest(owner.owner().0.as_bytes()).0;
        let tx = db.connection.transaction().map_err(storage_error)?;
        if let Some((stored_owner, session, stored_request, workspace, run)) = tx
            .query_row(
                "SELECT owner,session,request_digest,workspace,run
                 FROM workbench_run_commands WHERE command=?1",
                [&command.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?
        {
            if stored_owner != owner_digest
                || session != request.session.0
                || stored_request != request_digest.0
            {
                return Err(AuthError::Conflict);
            }
            let workspace = p::WorkspaceRef(workspace);
            let submission = p::WorkbenchRunSubmission {
                schema_version: p::SchemaVersion(1),
                session: p::SessionId(request.session.0.clone()),
                workspace: workspace.clone(),
                request: request.clone(),
            };
            submission
                .validate()
                .map_err(|_| AuthError::RecoveryRequired)?;
            tx.commit().map_err(storage_error)?;
            return Ok(WorkbenchRunBinding {
                workspace,
                run: run.map(p::RunId),
            });
        }

        let (archived, associated_workspace) = tx
            .query_row(
                "SELECT archived,workspace FROM workbench_sessions
                 WHERE owner=?1 AND session=?2",
                rusqlite::params![owner_digest, request.session.0],
                |row| Ok((row.get::<_, i64>(0)? != 0, row.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(AuthError::InvalidInput)?;
        if archived {
            return Err(AuthError::Conflict);
        }
        let workspace = associated_workspace
            .map(p::WorkspaceRef)
            .unwrap_or_else(|| default_workspace.clone());
        p::WorkbenchRunSubmission {
            schema_version: p::SchemaVersion(1),
            session: p::SessionId(request.session.0.clone()),
            workspace: workspace.clone(),
            request: request.clone(),
        }
        .validate()
        .map_err(|_| AuthError::RecoveryRequired)?;
        tx.execute(
            "INSERT INTO workbench_run_commands
             (command,owner,session,request_digest,workspace,run,created)
             VALUES(?1,?2,?3,?4,?5,NULL,?6)",
            rusqlite::params![
                command.0,
                owner_digest,
                request.session.0,
                request_digest.0,
                workspace.0,
                now(),
            ],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(WorkbenchRunBinding {
            workspace,
            run: None,
        })
    }

    pub(crate) fn record_workbench_run(
        &self,
        owner: &AuthenticatedWebOwner,
        request: &p::RunRequest,
        run: &p::RunId,
    ) -> AuthResult<()> {
        let command = request
            .idempotency_key
            .as_ref()
            .ok_or(AuthError::InvalidInput)?;
        if run.0.trim().is_empty() {
            return Err(AuthError::InvalidInput);
        }
        let request_digest =
            p::canonical_digest(&(p::SchemaVersion(1), "forme-web-workbench-run-v1", request))
                .map_err(|_| AuthError::Unavailable)?;
        let mut db = self.database()?;
        storage::check_owner(&db.connection, owner, now())?;
        let owner_digest = p::sha256_content_digest(owner.owner().0.as_bytes()).0;
        let tx = db.connection.transaction().map_err(storage_error)?;
        let stored = tx
            .query_row(
                "SELECT owner,session,request_digest,run FROM workbench_run_commands
                 WHERE command=?1",
                [&command.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(AuthError::RecoveryRequired)?;
        if stored.0 != owner_digest
            || stored.1 != request.session.0
            || stored.2 != request_digest.0
            || stored.3.as_ref().is_some_and(|existing| existing != &run.0)
        {
            return Err(AuthError::Conflict);
        }
        tx.execute(
            "UPDATE workbench_run_commands SET run=?1 WHERE command=?2 AND owner=?3
             AND session=?4 AND request_digest=?5 AND run IS NULL",
            rusqlite::params![
                run.0,
                command.0,
                owner_digest,
                request.session.0,
                request_digest.0
            ],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }

    pub(crate) fn list_workbench_sessions(
        &self,
        owner: &AuthenticatedWebOwner,
    ) -> AuthResult<p::WorkbenchSessionList> {
        let db = self.database()?;
        storage::check_owner(&db.connection, owner, now())?;
        let owner_digest = p::sha256_content_digest(owner.owner().0.as_bytes()).0;
        let mut statement = db
            .connection
            .prepare(
                "SELECT session,title,workspace,archived,created,updated
                 FROM workbench_sessions WHERE owner=?1 ORDER BY archived,updated DESC,session",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([owner_digest], |row| {
                Ok(p::WorkbenchSessionRecord {
                    schema_version: p::SchemaVersion(1),
                    session: p::SessionId(row.get(0)?),
                    title: row.get(1)?,
                    workspace: row.get::<_, Option<String>>(2)?.map(p::WorkspaceRef),
                    archived: row.get::<_, i64>(3)? != 0,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(storage_error)?;
        let mut sessions = Vec::new();
        for row in rows {
            let record = row.map_err(storage_error)?;
            record.validate().map_err(|_| AuthError::RecoveryRequired)?;
            sessions.push(record);
        }
        Ok(p::WorkbenchSessionList {
            schema_version: p::SchemaVersion(1),
            sessions,
        })
    }

    pub(crate) fn create_workbench_session(
        &self,
        owner: &AuthenticatedWebOwner,
        command: p::WorkbenchSessionCreate,
    ) -> AuthResult<p::WorkbenchSessionRecord> {
        command.validate().map_err(|_| AuthError::InvalidInput)?;
        let mut db = self.database()?;
        storage::check_owner(&db.connection, owner, now())?;
        let owner_digest = p::sha256_content_digest(owner.owner().0.as_bytes()).0;
        let binding = p::canonical_digest(&(
            p::SchemaVersion(1),
            "workbench-session-create",
            &owner_digest,
            &command.title,
            &command.workspace,
        ))
        .map_err(|_| AuthError::Unavailable)?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        if let Some((stored_owner, stored_binding, response)) = tx
            .query_row(
                "SELECT owner,binding,response FROM workbench_session_commands WHERE command=?1",
                [&command.command.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?
        {
            if stored_owner != owner_digest || stored_binding != binding.0 {
                return Err(AuthError::Conflict);
            }
            return serde_json::from_str(&response).map_err(|_| AuthError::RecoveryRequired);
        }
        let created = now();
        let session_digest = p::canonical_digest(&(
            p::SchemaVersion(1),
            "workbench-session",
            &owner_digest,
            &command.command,
        ))
        .map_err(|_| AuthError::Unavailable)?;
        let record = p::WorkbenchSessionRecord {
            schema_version: p::SchemaVersion(1),
            session: p::SessionId(format!("session:{}", session_digest.0)),
            title: command.title,
            workspace: command.workspace,
            archived: false,
            created_at: created,
            updated_at: created,
        };
        record.validate().map_err(|_| AuthError::RecoveryRequired)?;
        let response = serde_json::to_string(&record).map_err(|_| AuthError::Unavailable)?;
        tx.execute(
            "INSERT INTO workbench_sessions(session,owner,title,workspace,archived,created,updated)
             VALUES(?1,?2,?3,?4,0,?5,?5)",
            rusqlite::params![
                record.session.0,
                owner_digest,
                record.title,
                record.workspace.as_ref().map(|value| &value.0),
                created,
            ],
        )
        .map_err(storage_error)?;
        tx.execute(
            "INSERT INTO workbench_session_commands(command,owner,binding,response) VALUES(?1,?2,?3,?4)",
            rusqlite::params![command.command.0, owner_digest, binding.0, response],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }

    pub(crate) fn update_workbench_session(
        &self,
        owner: &AuthenticatedWebOwner,
        command: p::WorkbenchSessionUpdate,
    ) -> AuthResult<p::WorkbenchSessionRecord> {
        command.validate().map_err(|_| AuthError::InvalidInput)?;
        let mut db = self.database()?;
        storage::check_owner(&db.connection, owner, now())?;
        let owner_digest = p::sha256_content_digest(owner.owner().0.as_bytes()).0;
        let binding = p::canonical_digest(&(
            p::SchemaVersion(1),
            "workbench-session-update",
            &owner_digest,
            &command.session,
            &command.expected_updated_at,
            &command.mutation,
        ))
        .map_err(|_| AuthError::Unavailable)?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        if let Some((stored_owner, stored_binding, response)) = tx
            .query_row(
                "SELECT owner,binding,response FROM workbench_session_commands WHERE command=?1",
                [&command.command.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?
        {
            if stored_owner != owner_digest || stored_binding != binding.0 {
                return Err(AuthError::Conflict);
            }
            return serde_json::from_str(&response).map_err(|_| AuthError::RecoveryRequired);
        }
        let mut record = tx
            .query_row(
                "SELECT session,title,workspace,archived,created,updated FROM workbench_sessions
                 WHERE session=?1 AND owner=?2",
                rusqlite::params![command.session.0, owner_digest],
                |row| {
                    Ok(p::WorkbenchSessionRecord {
                        schema_version: p::SchemaVersion(1),
                        session: p::SessionId(row.get(0)?),
                        title: row.get(1)?,
                        workspace: row.get::<_, Option<String>>(2)?.map(p::WorkspaceRef),
                        archived: row.get::<_, i64>(3)? != 0,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(AuthError::InvalidInput)?;
        if record.updated_at != command.expected_updated_at {
            return Err(AuthError::Conflict);
        }
        match command.mutation {
            p::WorkbenchSessionMutation::Rename { title } => record.title = title,
            p::WorkbenchSessionMutation::Archive { archived } => record.archived = archived,
            p::WorkbenchSessionMutation::AssociateWorkspace { workspace } => {
                record.workspace = workspace
            }
        }
        record.updated_at = now().max(
            record
                .updated_at
                .checked_add(1)
                .ok_or(AuthError::Unavailable)?,
        );
        record.validate().map_err(|_| AuthError::RecoveryRequired)?;
        let response = serde_json::to_string(&record).map_err(|_| AuthError::Unavailable)?;
        tx.execute(
            "UPDATE workbench_sessions SET title=?1,workspace=?2,archived=?3,updated=?4
             WHERE session=?5 AND owner=?6 AND updated=?7",
            rusqlite::params![
                record.title,
                record.workspace.as_ref().map(|value| &value.0),
                i64::from(record.archived),
                record.updated_at,
                record.session.0,
                owner_digest,
                command.expected_updated_at,
            ],
        )
        .map_err(storage_error)?;
        tx.execute(
            "INSERT INTO workbench_session_commands(command,owner,binding,response) VALUES(?1,?2,?3,?4)",
            rusqlite::params![command.command.0, owner_digest, binding.0, response],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }
}
