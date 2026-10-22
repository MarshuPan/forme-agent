//! Operation-bound, one-shot password confirmation for product maintenance.
use super::{
    storage, storage_error, AuthResult, AuthenticatedWebOwner, SensitivePassword, WebIdentityStore,
};
use forme_protocol as p;

impl WebIdentityStore {
    /// Transport supplies the exact server-derived operation digest. The proof
    /// is consumed durably before any host IO; it never becomes an admin mode.
    pub(crate) fn authorize_product_operation(
        &self,
        owner: &AuthenticatedWebOwner,
        password: SensitivePassword,
        operation: &p::SchemaDigest,
    ) -> AuthResult<()> {
        let proof = self.reauthenticate(owner, password, operation)?;
        let mut db = self.database()?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        let now = super::now();
        storage::check_owner(&tx, owner, now)?;
        storage::claim_proof(&tx, owner, &proof, operation, now)?;
        tx.commit().map_err(storage_error)
    }
}
