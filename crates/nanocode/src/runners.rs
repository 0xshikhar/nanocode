pub use nanocode_code_mode::attachment::{
    AttachmentError, Runner, RunnerEvent, RunnerHandle, RunnerInfo, RunnerPool, RunnerServer,
    RunnerServerConfig, RunnerStatus,
};

impl crate::Nanocodex {
    #[must_use]
    pub fn runner_pool(&self) -> RunnerPool {
        self.runner_pool_ref().clone()
    }

    pub async fn initialize_runners(&self) -> Result<(), AttachmentError> {
        self.runner_agents_ref().initialize().await
    }

    #[must_use]
    pub fn runner_servers(&self) -> Vec<RunnerServer> {
        self.runner_pool_ref().servers()
    }

    pub async fn connect_runner(&self, runner: Runner) -> Result<RunnerHandle, AttachmentError> {
        self.runner_pool_ref().connect(runner).await
    }

    pub async fn listen_for_runners(
        &self,
        config: RunnerServerConfig,
    ) -> Result<RunnerServer, AttachmentError> {
        self.runner_pool_ref().listen(config).await
    }

    #[must_use]
    pub fn runner_events(&self) -> tokio::sync::mpsc::UnboundedReceiver<RunnerEvent> {
        self.runner_pool_ref().subscribe()
    }

    #[must_use]
    pub fn list_runners(&self) -> Vec<RunnerInfo> {
        self.runner_pool_ref().list()
    }

    pub async fn disconnect_runner(&self, name: &str) -> Result<(), AttachmentError> {
        self.runner_pool_ref().disconnect(name).await
    }
}

use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::{mpsc, oneshot};

pub(crate) struct RunnerAgents {
    pool: RunnerPool,
    state: Mutex<RunnerAgentState>,
    initialized: tokio::sync::OnceCell<Result<(), AttachmentError>>,
}

struct RunnerAgentState {
    events: Option<mpsc::UnboundedReceiver<RunnerEvent>>,
    agents: Vec<(String, crate::WeakAgent, mpsc::UnboundedSender<Notice>)>,
    runners: Vec<Runner>,
    servers: Vec<RunnerServerConfig>,
    startup: Vec<tokio::sync::watch::Receiver<Option<Result<(), AttachmentError>>>>,
}

struct Notice {
    text: String,
    completed: Option<oneshot::Sender<nanocode_agent::Result<()>>>,
}

impl RunnerAgents {
    pub(crate) fn new(
        pool: RunnerPool,
        runners: Vec<Runner>,
        servers: Vec<RunnerServerConfig>,
    ) -> Arc<Self> {
        let events = pool.subscribe();
        Arc::new(Self {
            pool,
            initialized: tokio::sync::OnceCell::new(),
            state: Mutex::new(RunnerAgentState {
                events: Some(events),
                agents: Vec::new(),
                runners,
                servers,
                startup: Vec::new(),
            }),
        })
    }

    pub(crate) fn activate(self: &Arc<Self>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let (events, runners, servers) = (
            state.events.take(),
            std::mem::take(&mut state.runners),
            std::mem::take(&mut state.servers),
        );
        if let Some(mut events) = events {
            let weak = Arc::downgrade(self);
            runtime.spawn(async move {
                while let Some(event) = events.recv().await {
                    let Some(agents) = weak.upgrade() else { break };
                    agents.notify(event);
                }
            });
        }
        for runner in runners {
            if let Err(error) = self.pool.start(runner) {
                state
                    .startup
                    .push(tokio::sync::watch::channel(Some(Err(error))).1);
            }
        }
        for server in servers {
            let pool = self.pool.clone();
            let (receipt, result) = tokio::sync::watch::channel(None);
            runtime.spawn(async move {
                let outcome = match pool.listen(server).await {
                    Ok(_) => Ok(()),
                    Err(error) => {
                        pool.report_failure("runner-server", error.to_string());
                        Err(error)
                    }
                };
                receipt.send_replace(Some(outcome));
            });
            state.startup.push(result);
        }
    }

    pub(crate) async fn initialize(self: &Arc<Self>) -> Result<(), AttachmentError> {
        self.activate();
        self.initialized
            .get_or_init(|| async {
                let startup = self
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .startup
                    .clone();
                let mut result = Ok(());
                for mut receipt in startup {
                    let outcome = loop {
                        if let Some(outcome) = receipt.borrow().clone() {
                            break outcome;
                        }
                        if receipt.changed().await.is_err() {
                            break Err(AttachmentError::Transport(
                                "runner startup stopped before completion".into(),
                            ));
                        }
                    };
                    if result.is_ok() {
                        result = outcome;
                    }
                }
                result
            })
            .await
            .clone()
    }

    fn summary(&self) -> String {
        let runners = self.pool.list();
        let mounts = runners
            .iter()
            .filter(|runner| runner.status == RunnerStatus::Ready)
            .map(|runner| {
                format!(
                    "/{} (mode: {:?}, workspace: {}, tools: {})",
                    runner.name,
                    runner.mode,
                    runner.workspace.as_ref().map_or_else(
                        || "unspecified".to_owned(),
                        |path| path.display().to_string()
                    ),
                    runner.tool_count,
                )
            })
            .collect::<Vec<_>>();
        if mounts.is_empty() {
            "Runner mounts: no remote runners are currently available.".to_owned()
        } else {
            format!("Runner mounts currently available: {}.", mounts.join("; "))
        }
    }

    fn notify(&self, event: RunnerEvent) {
        let notice = match event {
            RunnerEvent::StatusChanged { info } => match info.status {
                RunnerStatus::Ready | RunnerStatus::Disconnected | RunnerStatus::Failed(_) => {
                    format!(
                        "Runner {} status: {:?}. {}",
                        info.name,
                        info.status,
                        self.summary()
                    )
                }
                RunnerStatus::Connecting => return,
            },
            RunnerEvent::Failed { name, error } => {
                format!("Runner {name} failed: {error}. {}", self.summary())
            }
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.agents.retain(|(_, agent, notices)| {
            agent.is_live()
                && notices
                    .send(Notice {
                        text: notice.clone(),
                        completed: None,
                    })
                    .is_ok()
        });
    }

    pub(crate) async fn register(
        self: &Arc<Self>,
        agent: &crate::Agent,
        await_initial: bool,
    ) -> nanocode_agent::Result<()> {
        self.activate();
        let weak = agent.downgrade();
        let worker = weak.clone();
        let (notices, mut receiver) = mpsc::unbounded_channel::<Notice>();
        let (completed, result) = oneshot::channel();
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .agents
                .retain(|(_, agent, notices)| agent.is_live() && !notices.is_closed());
            if state
                .agents
                .iter()
                .any(|(id, _, _)| id == agent.session_id())
            {
                return Ok(());
            }
            notices
                .send(Notice {
                    text: self.summary(),
                    completed: await_initial.then_some(completed),
                })
                .map_err(|_| crate::NanocodexError::AgentStopped)?;
            state
                .agents
                .push((agent.session_id().to_owned(), weak, notices));
        }
        tokio::spawn(async move {
            while let Some(notice) = receiver.recv().await {
                let outcome = worker
                    .append_developer_message(notice.text)
                    .await
                    .unwrap_or(Err(crate::NanocodexError::AgentStopped))
                    .map(|_| ());
                let failed = outcome.is_err();
                if let Some(completed) = notice.completed {
                    let _ = completed.send(outcome);
                }
                if failed {
                    break;
                }
            }
        });
        if await_initial {
            result
                .await
                .map_err(|_| crate::NanocodexError::AgentStopped)?
        } else {
            Ok(())
        }
    }
}
