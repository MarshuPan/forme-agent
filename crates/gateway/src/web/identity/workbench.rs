//! Durable Web workbench session metadata, separate from brain state.
use rusqlite::OptionalExtension;

use super::{
    now, storage, storage_error, AuthError, AuthResult, AuthenticatedWebOwner, WebIdentityStore,
};
use forme_protocol as p;

impl WebIdentityStore {
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
        record.updated_at = now();
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
