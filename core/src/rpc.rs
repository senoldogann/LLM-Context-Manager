use crate::engine::{CursorPosition, RetrievalEngine};
use tonic::{Request, Response, Status};

// Import generated proto module
pub mod ccm_proto {
    tonic::include_proto!("ccm");
}

use ccm_proto::context_manager_server::{ContextManager, ContextManagerServer};
use ccm_proto::{ContextItem, ContextRequest, ContextResponse};

pub struct MyContextManager {
    engine: std::sync::Arc<RetrievalEngine>,
}

impl MyContextManager {
    pub fn new(engine: std::sync::Arc<RetrievalEngine>) -> Self {
        Self { engine }
    }
}

#[tonic::async_trait]
impl ContextManager for MyContextManager {
    async fn get_context(
        &self,
        request: Request<ContextRequest>,
    ) -> Result<Response<ContextResponse>, Status> {
        let req = request.into_inner();

        let cursor = CursorPosition {
            file_path: req.file_path,
            line: req.line as usize,
            column: req.column as usize,
        };

        match self.engine.predict_context(&cursor).await {
            Ok(suggestions) => {
                let items: Vec<ContextItem> = suggestions
                    .into_iter()
                    .map(|s| ContextItem {
                        title: s.title,
                        content: s.content,
                        relevance_score: s.relevance_score,
                        reason: s.reason,
                    })
                    .collect();

                Ok(Response::new(ContextResponse { suggestions: items }))
            }
            Err(e) => Err(Status::internal(format!("Engine error: {}", e))),
        }
    }
}

// Function to create the server service
pub fn create_service(
    engine: std::sync::Arc<RetrievalEngine>,
) -> ContextManagerServer<MyContextManager> {
    ContextManagerServer::new(MyContextManager::new(engine))
}
