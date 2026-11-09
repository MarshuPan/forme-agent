//! Built-in execution capability assembly for managed and development entry points.
use super::*;

pub(super) struct Inputs<'a> {
    pub config: &'a ProductionRuntimeConfig,
    pub workspace: &'a p::WorkspaceRef,
    pub protection: forme_store::StoreContentProtection,
    pub store: SqliteEventStore,
    pub backends: Arc<ExecutionBackendRegistry>,
    pub contents: Arc<dyn ContentResolver>,
    pub managed_mode: bool,
    pub worker_binding: Option<&'a ManagedWorkerBinding>,
    pub desktop_binding: Option<&'a ManagedWorkerBinding>,
}

pub(super) struct BuiltinExecution {
    pub bindings: BTreeMap<p::CapabilityRef, CapabilityBinding>,
    pub entries: Vec<CapabilityDescriptor>,
    pub tool_schemas: BTreeMap<p::ToolRef, String>,
    pub service_states: Vec<RuntimeServiceState>,
    pub browser_active: bool,
    pub computer_active: bool,
    pub desktop_backend: Option<Arc<WorkerActionBackend>>,
    pub pty_active: bool,
}

pub(super) fn assemble(inputs: Inputs<'_>) -> p::Result<BuiltinExecution> {
    let Inputs {
        config,
        workspace,
        protection,
        store,
        backends,
        contents,
        managed_mode,
        worker_binding,
        desktop_binding,
    } = inputs;
    let capability_config = &config.capabilities;
    let output_budget = OutputBudget::truncate_at(capability_config.output_budget_bytes);
    let timeout = capability_config.timeout;
    let rejecting_secrets: Arc<dyn SecretResolver> = Arc::new(RejectingSecretResolver);
    let mut bindings = BTreeMap::new();
    let mut builtin_entries = Vec::new();
    let mut tool_schemas = BTreeMap::new();
    let mut service_states = Vec::new();
    let mut browser_active = false;
    let mut computer_active = false;
    let mut desktop_backend = None;
    let mut pty_active = false;

    if capability_config.notification_enabled {
        let ready = if managed_mode {
            try_register_managed_worker_backend(
                managed_mode,
                worker_binding,
                p::BackendKind::Notification,
                config,
                &output_budget,
                timeout,
                &backends,
            )?
        } else {
            backends.register(Arc::new(NotificationBackend::new(
                Arc::new(forme_execution::ConsoleNotificationSink),
                output_budget.clone(),
                timeout,
            )))?;
            true
        };
        if ready {
            add_builtin_capability(
                &mut builtin_entries,
                &mut bindings,
                &mut tool_schemas,
                p::CapabilityRef("capability:local-notification".into()),
                Capability::Tool(p::ToolRef("tool:local-notification".into())),
                p::PermissionRef("permission:local-notification".into()),
                workspace,
                p::Risk::High,
                p::BackendKind::Notification,
                "notification",
            );
            service_states.push(RuntimeServiceState::ready("notification-backend"));
        } else if managed_mode {
            service_states.push(RuntimeServiceState::unavailable(
                "notification-backend",
                RuntimeServiceStatus::Blocking,
                "managed worker binding is missing or unavailable",
            ));
        }
    } else {
        service_states.push(RuntimeServiceState::unavailable(
            "notification-backend",
            RuntimeServiceStatus::Disabled,
            "notification disabled by explicit configuration",
        ));
    }

    if capability_config.file.enabled {
        let ready = if managed_mode {
            try_register_managed_worker_backend(
                managed_mode,
                worker_binding,
                p::BackendKind::File,
                config,
                &output_budget,
                timeout,
                &backends,
            )?
        } else {
            match FileBackend::new(
                capability_config.file.roots.clone(),
                output_budget.clone(),
                timeout,
            ) {
                Ok(backend) => {
                    backends.register(Arc::new(backend))?;
                    true
                }
                Err(_) => false,
            }
        };
        if ready {
            add_builtin_capability(
                &mut builtin_entries,
                &mut bindings,
                &mut tool_schemas,
                p::CapabilityRef("capability:file".into()),
                Capability::Tool(p::ToolRef("tool:file".into())),
                p::PermissionRef("permission:file".into()),
                workspace,
                p::Risk::Medium,
                p::BackendKind::File,
                "file",
            );
            service_states.push(RuntimeServiceState::ready("file-backend"));
        } else if managed_mode {
            service_states.push(RuntimeServiceState::unavailable(
                "file-backend",
                RuntimeServiceStatus::Blocking,
                "managed worker binding is missing or unavailable",
            ));
        } else {
            service_states.push(RuntimeServiceState::unavailable(
                "file-backend",
                RuntimeServiceStatus::Unsupported,
                "configured file roots are unavailable",
            ));
        }
    } else {
        service_states.push(RuntimeServiceState::unavailable(
            "file-backend",
            RuntimeServiceStatus::Disabled,
            "file backend disabled by explicit configuration",
        ));
    }

    if capability_config.shell.enabled {
        let ready = if managed_mode {
            try_register_managed_worker_backend(
                managed_mode,
                worker_binding,
                p::BackendKind::Shell,
                config,
                &output_budget,
                timeout,
                &backends,
            )?
        } else {
            let sandbox = ShellSandbox {
                schema_version: p::SchemaVersion(1),
                available: true,
                allowed_programs: capability_config.shell.allowed_programs.clone(),
                allowed_roots: capability_config.shell.allowed_roots.clone(),
                environment: capability_config.shell.environment.clone(),
                network_allowed: capability_config.shell.network_allowed,
            };
            match ShellBackend::new(sandbox, output_budget.clone(), timeout) {
                Ok(backend) => {
                    backends.register(Arc::new(backend))?;
                    true
                }
                Err(_) => false,
            }
        };
        if ready {
            add_builtin_capability(
                &mut builtin_entries,
                &mut bindings,
                &mut tool_schemas,
                p::CapabilityRef("capability:shell".into()),
                Capability::Tool(p::ToolRef("tool:shell".into())),
                p::PermissionRef("permission:shell".into()),
                workspace,
                p::Risk::High,
                p::BackendKind::Shell,
                "shell",
            );
            service_states.push(RuntimeServiceState::ready("shell-backend"));
        } else if managed_mode {
            service_states.push(RuntimeServiceState::unavailable(
                "shell-backend",
                RuntimeServiceStatus::Blocking,
                "managed worker binding is missing or unavailable",
            ));
        } else {
            service_states.push(RuntimeServiceState::unavailable(
                "shell-backend",
                RuntimeServiceStatus::Unsupported,
                "configured shell roots or programs are unavailable",
            ));
        }
    } else {
        service_states.push(RuntimeServiceState::unavailable(
            "shell-backend",
            RuntimeServiceStatus::Disabled,
            "shell backend disabled by explicit configuration",
        ));
    }

    let artifact_store = if capability_config.browser.enabled || capability_config.computer.enabled
    {
        FileArtifactStore::new(config.data.artifact_root.clone())
            .ok()
            .map(|inner| {
                Arc::new(ProtectedRuntimeArtifacts {
                    inner,
                    protection: protection.clone(),
                    store: store.clone(),
                    owner: p::VerifiedPrincipal(config.owner.0.clone()),
                }) as Arc<dyn forme_execution::ArtifactStore>
            })
    } else {
        None
    };

    if managed_mode && capability_config.browser.enabled {
        let ready = try_register_managed_worker_backend(
            managed_mode,
            worker_binding,
            p::BackendKind::Browser,
            config,
            &output_budget,
            timeout,
            &backends,
        )?;
        if ready {
            add_builtin_capability(
                &mut builtin_entries,
                &mut bindings,
                &mut tool_schemas,
                p::CapabilityRef("capability:browser".into()),
                Capability::Tool(p::ToolRef("tool:browser".into())),
                p::PermissionRef("permission:browser".into()),
                workspace,
                p::Risk::High,
                p::BackendKind::Browser,
                "browser",
            );
            browser_active = true;
            service_states.push(RuntimeServiceState::ready("browser-backend"));
        } else {
            service_states.push(RuntimeServiceState::unavailable(
                "browser-backend",
                RuntimeServiceStatus::Blocking,
                "managed worker binding is missing or unavailable",
            ));
        }
    } else if capability_config.browser.enabled
        && capability_config.browser.driver.0 == "browser:chrome"
        && capability_config.browser.executable.is_some()
        && !capability_config.browser.allowed_origins.is_empty()
    {
        match (
            capability_config.browser.executable.clone(),
            artifact_store.clone(),
        ) {
            (Some(executable), Some(artifacts)) => {
                let driver =
                    HeadlessChromeDriver::new(executable, contents.clone(), artifacts, timeout);
                match driver.and_then(|driver| {
                    BrowserBackend::new(
                        capability_config.browser.driver.clone(),
                        Arc::new(driver),
                        rejecting_secrets.clone(),
                        output_budget.clone(),
                        timeout,
                    )
                }) {
                    Ok(backend) => {
                        backends.register(Arc::new(backend))?;
                        add_builtin_capability(
                            &mut builtin_entries,
                            &mut bindings,
                            &mut tool_schemas,
                            p::CapabilityRef("capability:browser".into()),
                            Capability::Tool(p::ToolRef("tool:browser".into())),
                            p::PermissionRef("permission:browser".into()),
                            workspace,
                            p::Risk::High,
                            p::BackendKind::Browser,
                            "browser",
                        );
                        browser_active = true;
                        service_states.push(RuntimeServiceState::ready("browser-backend"));
                    }
                    Err(_) => service_states.push(RuntimeServiceState::unavailable(
                        "browser-backend",
                        RuntimeServiceStatus::Unsupported,
                        "browser driver or artifact store is unavailable",
                    )),
                }
            }
            _ => service_states.push(RuntimeServiceState::unavailable(
                "browser-backend",
                RuntimeServiceStatus::Unsupported,
                "browser driver or artifact store is unavailable",
            )),
        }
    } else if capability_config.browser.enabled {
        service_states.push(RuntimeServiceState::unavailable(
            "browser-backend",
            RuntimeServiceStatus::Unsupported,
            "browser driver, executable, or allowed origins are unavailable",
        ));
    } else {
        service_states.push(RuntimeServiceState::unavailable(
            "browser-backend",
            RuntimeServiceStatus::Disabled,
            "browser backend disabled by explicit configuration",
        ));
    }

    let computer_surfaces_valid = !capability_config.computer.surfaces.is_empty()
        && capability_config.computer.surfaces.iter().all(|surface| {
            surface.schema_version.0 > 0
                && !surface.surface.0.trim().is_empty()
                && surface.bounds.validate().is_ok()
        });
    if managed_mode && capability_config.computer.enabled {
        desktop_backend = managed_workers::register_computer(
            desktop_binding,
            config,
            &output_budget,
            timeout,
            &backends,
            artifact_store.clone(),
        )?;
        if desktop_backend.is_some() {
            add_builtin_capability(
                &mut builtin_entries,
                &mut bindings,
                &mut tool_schemas,
                p::CapabilityRef("capability:computer".into()),
                Capability::Tool(p::ToolRef("tool:computer".into())),
                p::PermissionRef("permission:computer".into()),
                workspace,
                p::Risk::High,
                p::BackendKind::Computer,
                "computer",
            );
            computer_active = true;
            service_states.push(RuntimeServiceState::ready("computer-backend"));
        } else {
            service_states.push(RuntimeServiceState::unavailable(
                "computer-backend",
                RuntimeServiceStatus::Blocking,
                "managed worker binding is missing or unavailable",
            ));
        }
    } else if capability_config.computer.enabled
        && capability_config.computer.driver.0 == "computer:native"
        && computer_surfaces_valid
    {
        match artifact_store.clone().map(|artifacts| {
            build_computer_backend(
                &capability_config.computer,
                contents.clone(),
                artifacts,
                rejecting_secrets.clone(),
                output_budget.clone(),
                timeout,
            )
        }) {
            Some(Ok(backend)) => {
                backends.register(Arc::new(backend))?;
                add_builtin_capability(
                    &mut builtin_entries,
                    &mut bindings,
                    &mut tool_schemas,
                    p::CapabilityRef("capability:computer".into()),
                    Capability::Tool(p::ToolRef("tool:computer".into())),
                    p::PermissionRef("permission:computer".into()),
                    workspace,
                    p::Risk::High,
                    p::BackendKind::Computer,
                    "computer",
                );
                computer_active = true;
                service_states.push(RuntimeServiceState::ready("computer-backend"));
            }
            _ => service_states.push(RuntimeServiceState::unavailable(
                "computer-backend",
                RuntimeServiceStatus::Unsupported,
                "computer driver or artifact store is unavailable",
            )),
        }
    } else if capability_config.computer.enabled {
        service_states.push(RuntimeServiceState::unavailable(
            "computer-backend",
            RuntimeServiceStatus::Unsupported,
            "computer driver, surfaces, or artifact store are unavailable",
        ));
    } else {
        service_states.push(RuntimeServiceState::unavailable(
            "computer-backend",
            RuntimeServiceStatus::Disabled,
            "computer backend disabled by explicit configuration",
        ));
    }

    if capability_config.pty.enabled {
        let ready = if managed_mode {
            try_register_managed_worker_backend(
                managed_mode,
                worker_binding,
                p::BackendKind::Pty,
                config,
                &output_budget,
                timeout,
                &backends,
            )?
        } else {
            match PtyBackend::new(
                capability_config.pty.allowed_programs.clone(),
                capability_config.pty.allowed_roots.clone(),
                rejecting_secrets.clone(),
                contents.clone(),
                output_budget.clone(),
                timeout,
            ) {
                Ok(backend) => {
                    backends.register(Arc::new(backend))?;
                    true
                }
                Err(_) => false,
            }
        };
        if ready {
            add_builtin_capability(
                &mut builtin_entries,
                &mut bindings,
                &mut tool_schemas,
                p::CapabilityRef("capability:pty".into()),
                Capability::Tool(p::ToolRef("tool:pty".into())),
                p::PermissionRef("permission:pty".into()),
                workspace,
                p::Risk::High,
                p::BackendKind::Pty,
                "pty",
            );
            pty_active = true;
            service_states.push(RuntimeServiceState::ready("pty-backend"));
        } else if managed_mode {
            service_states.push(RuntimeServiceState::unavailable(
                "pty-backend",
                RuntimeServiceStatus::Blocking,
                "managed worker binding is missing or unavailable",
            ));
        } else {
            service_states.push(RuntimeServiceState::unavailable(
                "pty-backend",
                RuntimeServiceStatus::Unsupported,
                "configured PTY programs or roots are unavailable",
            ));
        }
    } else {
        service_states.push(RuntimeServiceState::unavailable(
            "pty-backend",
            RuntimeServiceStatus::Disabled,
            "PTY backend disabled by explicit configuration",
        ));
    }

    Ok(BuiltinExecution {
        bindings,
        entries: builtin_entries,
        tool_schemas,
        service_states,
        browser_active,
        computer_active,
        desktop_backend,
        pty_active,
    })
}
