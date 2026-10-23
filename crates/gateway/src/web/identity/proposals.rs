//! Durable owner-bound recovery of ordinary proposal commands, separate from brain facts.
use rusqlite::OptionalExtension;

use super::{
    now, storage, storage_error, AuthError, AuthResult, AuthenticatedWebOwner, WebIdentityStore,
};
use forme_protocol as p;

const MAX_PENDING_PROPOSAL_COMMANDS: i64 = 256;

impl WebIdentityStore {
    pub(crate) fn prepare_workbench_proposal(
        &self,
        owner: &AuthenticatedWebOwner,
        command: &p::WorkbenchProposalCommand,
    ) -> AuthResult<()> {
        command.validate().map_err(|_| AuthError::InvalidInput)?;
        let owner_digest = p::sha256_content_digest(owner.owner().0.as_bytes()).0;
        let binding = proposal_binding(owner, command)?;
        let payload = serde_json::to_string(command).map_err(|_| AuthError::Unavailable)?;
        let mut db = self.database()?;
        let tx = db
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        storage::check_owner(&tx, owner, now())?;

        if let Some((stored_owner, stored_binding, run, proposal, stored_payload)) = tx
            .query_row(
                "SELECT owner,binding,run,proposal,payload
                 FROM workbench_proposal_commands WHERE command=?1",
                [&command.command.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?
        {
            if stored_owner != owner_digest
                || stored_binding != binding.0
                || run != command.run.0
                || proposal != command.proposal.0
                || stored_payload != payload
            {
                return Err(AuthError::Conflict);
            }
            tx.commit().map_err(storage_error)?;
            return Ok(());
        }

        let target: Option<(String, String, String)> = tx
            .query_row(
                "SELECT command,binding,payload FROM workbench_proposal_commands
                 WHERE owner=?1 AND run=?2 AND proposal=?3",
                rusqlite::params![owner_digest, command.run.0, command.proposal.0],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if target.is_some() {
            return Err(AuthError::Conflict);
        }

        let pending: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM workbench_proposal_commands WHERE owner=?1",
                [&owner_digest],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if pending >= MAX_PENDING_PROPOSAL_COMMANDS {
            return Err(AuthError::Busy);
        }
        tx.execute(
            "INSERT INTO workbench_proposal_commands
             (command,owner,binding,run,proposal,payload,created)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                command.command.0,
                owner_digest,
                binding.0,
                command.run.0,
                command.proposal.0,
                payload,
                now(),
            ],
        )
        .map_err(|error| match error {
            rusqlite::Error::SqliteFailure(code, _)
                if code.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                AuthError::Conflict
            }
            _ => storage_error(error),
        })?;
        tx.commit().map_err(storage_error)
    }

    pub(crate) fn pending_workbench_proposals(
        &self,
        owner: &AuthenticatedWebOwner,
    ) -> AuthResult<Vec<p::WorkbenchProposalCommand>> {
        let db = self.database()?;
        storage::check_owner(&db.connection, owner, now())?;
        let owner_digest = p::sha256_content_digest(owner.owner().0.as_bytes()).0;
        let mut statement = db
            .connection
            .prepare(
                "SELECT command,binding,run,proposal,payload,created
                 FROM workbench_proposal_commands WHERE owner=?1
                 ORDER BY created,command LIMIT ?2",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(
                rusqlite::params![owner_digest, MAX_PENDING_PROPOSAL_COMMANDS + 1],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .map_err(storage_error)?;
        let mut commands = Vec::new();
        for row in rows {
            let (key, recorded_binding, run, proposal, payload, created) =
                row.map_err(storage_error)?;
            let command: p::WorkbenchProposalCommand =
                serde_json::from_str(&payload).map_err(|_| AuthError::RecoveryRequired)?;
            command
                .validate()
                .map_err(|_| AuthError::RecoveryRequired)?;
            if command.command.0 != key
                || command.run.0 != run
                || command.proposal.0 != proposal
                || created <= 0
                || proposal_binding(owner, &command)
                    .map_err(|_| AuthError::RecoveryRequired)?
                    .0
                    != recorded_binding
            {
                return Err(AuthError::RecoveryRequired);
            }
            commands.push(command);
            if commands.len() > MAX_PENDING_PROPOSAL_COMMANDS as usize {
                return Err(AuthError::RecoveryRequired);
            }
        }
        Ok(commands)
    }

    pub(crate) fn finish_workbench_proposal(
        &self,
        owner: &AuthenticatedWebOwner,
        command: &p::WorkbenchProposalCommand,
        receipt: &p::WorkbenchCommandReceipt,
    ) -> AuthResult<()> {
        command
            .validate()
            .map_err(|_| AuthError::RecoveryRequired)?;
        let binding = proposal_binding(owner, command)?;
        if receipt.schema_version != p::SchemaVersion(1)
            || receipt.command != command.command
            || receipt.binding != binding
            || receipt.status == p::WorkbenchCommandStatus::Unknown
        {
            return Err(AuthError::RecoveryRequired);
        }
        self.remove_workbench_proposal(owner, command, &binding)
    }

    pub(crate) fn reject_workbench_proposal_conflict(
        &self,
        owner: &AuthenticatedWebOwner,
        command: &p::WorkbenchProposalCommand,
    ) -> AuthResult<()> {
        command
            .validate()
            .map_err(|_| AuthError::RecoveryRequired)?;
        let binding = proposal_binding(owner, command)?;
        self.remove_workbench_proposal(owner, command, &binding)
    }

    fn remove_workbench_proposal(
        &self,
        owner: &AuthenticatedWebOwner,
        command: &p::WorkbenchProposalCommand,
        binding: &p::SchemaDigest,
    ) -> AuthResult<()> {
        let owner_digest = p::sha256_content_digest(owner.owner().0.as_bytes()).0;
        let mut db = self.database()?;
        let tx = db
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        storage::check_owner(&tx, owner, now())?;
        let stored: Option<(String, String, String, String)> = tx
            .query_row(
                "SELECT owner,binding,run,proposal FROM workbench_proposal_commands
                 WHERE command=?1",
                [&command.command.0],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some(stored) = stored {
            if stored
                != (
                    owner_digest.clone(),
                    binding.0.clone(),
                    command.run.0.clone(),
                    command.proposal.0.clone(),
                )
            {
                return Err(AuthError::Conflict);
            }
            tx.execute(
                "DELETE FROM workbench_proposal_commands
                 WHERE command=?1 AND owner=?2 AND binding=?3",
                rusqlite::params![command.command.0, owner_digest, binding.0],
            )
            .map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)
    }
}

fn proposal_binding(
    owner: &AuthenticatedWebOwner,
    command: &p::WorkbenchProposalCommand,
) -> AuthResult<p::SchemaDigest> {
    p::canonical_digest(&(p::VerifiedPrincipal(owner.owner().0.clone()), command))
        .map_err(|_| AuthError::Unavailable)
}
