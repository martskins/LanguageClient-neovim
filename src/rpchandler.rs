use super::*;
use crate::language_client::LanguageClient;
use crate::lsp::notification::Notification;
use crate::lsp::request::Request;

fn is_content_modified_error(err: &failure::Error) -> bool {
    match err.as_fail().downcast_ref::<LSError>() {
        Some(err) if err == &LSError::ContentModified => true,
        _ => false,
    }
}

impl LanguageClient {
    pub fn handle_call(&self, msg: Call) -> Fallible<()> {
        match msg {
            Call::MethodCall(lang_id, method_call) => {
                let result = self.handle_method_call(lang_id.as_deref(), &method_call);
                if let Err(ref err) = result {
                    if is_content_modified_error(err) {
                        return Ok(());
                    }

                    if err.find_root_cause().downcast_ref::<LCError>().is_none() {
                        error!(
                            "Error handling message: {}\n\nMessage: {}\n\nError: {:?}",
                            err,
                            serde_json::to_string(&method_call).unwrap_or_default(),
                            err
                        );
                    }
                }
                self.get_client(&lang_id)?
                    .output(method_call.id.to_int()?, result)?;
            }
            Call::Notification(lang_id, notification) => {
                let result = self.handle_notification(lang_id.as_deref(), &notification);
                if let Err(ref err) = result {
                    if is_content_modified_error(err) {
                        return Ok(());
                    }

                    if err.downcast_ref::<LCError>().is_none() {
                        error!(
                            "Error handling message: {}\n\nMessage: {}\n\nError: {:?}",
                            err,
                            serde_json::to_string(&notification).unwrap_or_default(),
                            err
                        );
                    }
                }
            }
        }

        // FIXME
        if let Err(err) = self.handle_fs_events() {
            warn!("{:?}", err);
        }

        Ok(())
    }

    pub fn handle_method_call(
        &self,
        language_id: Option<&str>,
        method_call: &rpc::MethodCall,
    ) -> Fallible<Value> {
        let params = serde_json::to_value(method_call.params.clone())?;

        let user_handler =
            self.get(|state| state.user_handlers.get(&method_call.method).cloned())?;
        if let Some(user_handler) = user_handler {
            return self.vim()?.rpcclient.call(&user_handler, params);
        }

        match method_call.method.as_str() {
            lsp::request::RegisterCapability::METHOD => {
                self.client_register_capability(language_id.unwrap_or_default(), &params)
            }
            lsp::request::UnregisterCapability::METHOD => {
                self.client_unregister_capability(language_id.unwrap_or_default(), &params)
            }
            lsp::request::HoverRequest::METHOD => self.text_document_hover(&params),
            lsp::request::Rename::METHOD => self.text_document_rename(&params),
            lsp::request::DocumentSymbolRequest::METHOD => {
                self.text_document_document_symbol(&params)
            }
            lsp::request::ShowMessageRequest::METHOD => self.window_show_message_request(&params),
            lsp::request::WorkspaceSymbol::METHOD => self.workspace_symbol(&params),
            lsp::request::CodeActionRequest::METHOD => self.text_document_code_action(&params),
            lsp::request::Completion::METHOD => self.text_document_completion(&params),
            lsp::request::SignatureHelpRequest::METHOD => {
                self.text_document_signature_help(&params)
            }
            lsp::request::References::METHOD => self.text_document_references(&params),
            lsp::request::Formatting::METHOD => self.text_document_formatting(&params),
            lsp::request::RangeFormatting::METHOD => self.text_document_range_formatting(&params),
            lsp::request::CodeLensRequest::METHOD => self.text_document_code_lens(&params),
            lsp::request::ResolveCompletionItem::METHOD => self.completion_item_resolve(&params),
            lsp::request::ExecuteCommand::METHOD => self.workspace_execute_command(&params),
            lsp::request::ApplyWorkspaceEdit::METHOD => self.workspace_apply_edit(&params),
            lsp::request::DocumentHighlightRequest::METHOD => {
                self.text_document_document_highlight(&params)
            }
            // Extensions.
            REQUEST__FindLocations => self.find_locations(&params),
            REQUEST__GetState => self.get_state(&params),
            REQUEST__IsAlive => self.is_alive(&params),
            REQUEST__StartServer => self.start_server(&params),
            REQUEST__RegisterServerCommands => self.register_server_commands(&params),
            REQUEST__SetLoggingLevel => self.set_logging_level(&params),
            REQUEST__SetDiagnosticsList => self.set_diagnostics_list(&params),
            REQUEST__RegisterHandlers => self.register_handlers(&params),
            REQUEST__NCMRefresh => self.ncm_refresh(&params),
            REQUEST__NCM2OnComplete => self.ncm2_on_complete(&params),
            REQUEST__ExplainErrorAtPoint => self.explain_error_at_point(&params),
            REQUEST__OmniComplete => self.omnicomplete(&params),
            REQUEST__ClassFileContents => self.java_class_file_contents(&params),
            REQUEST__DebugInfo => self.debug_info(&params),
            REQUEST__CodeLensAction => self.handle_code_lens_action(&params),
            REQUEST__SemanticScopes => self.semantic_scopes(&params),
            REQUEST__ShowSemanticHighlightSymbols => self.semantic_highlight_symbols(&params),

            _ => {
                let languageId_target = if language_id.is_some() {
                    // Message from language server. No handler found.
                    let msg = format!("Message not handled: {:?}", method_call);
                    if method_call.method.starts_with('$') {
                        warn!("{}", msg);
                        return Ok(Value::default());
                    } else {
                        return Err(err_msg(msg));
                    }
                } else {
                    // Message from vim. Proxy to language server.
                    let filename = self.vim()?.get_filename(&params)?;
                    let languageId_target = self.vim()?.get_language_id(&filename, &params)?;
                    info!(
                        "Proxy message directly to language server: {:?}",
                        method_call
                    );
                    Some(languageId_target)
                };

                self.get_client(&languageId_target)?
                    .call(&method_call.method, &params)
            }
        }
    }

    pub fn handle_notification(
        &self,
        languageId: Option<&str>,
        notification: &rpc::Notification,
    ) -> Fallible<()> {
        let params = serde_json::to_value(notification.params.clone())?;

        let user_handler =
            self.get(|state| state.user_handlers.get(&notification.method).cloned())?;
        if let Some(user_handler) = user_handler {
            return self.vim()?.rpcclient.notify(&user_handler, params);
        }

        match notification.method.as_str() {
            lsp::notification::DidChangeConfiguration::METHOD => {
                self.workspace_did_change_configuration(&params)?
            }
            lsp::notification::DidOpenTextDocument::METHOD => {
                self.text_document_did_open(&params)?
            }
            lsp::notification::DidChangeTextDocument::METHOD => {
                self.text_document_did_change(&params)?
            }
            lsp::notification::DidSaveTextDocument::METHOD => {
                self.text_document_did_save(&params)?
            }
            lsp::notification::DidCloseTextDocument::METHOD => {
                self.text_document_did_close(&params)?
            }
            lsp::notification::PublishDiagnostics::METHOD => {
                self.text_document_publish_diagnostics(&params)?
            }
            lsp::notification::SemanticHighlighting::METHOD => {
                self.text_document_semantic_highlight(&params)?
            }
            lsp::notification::Progress::METHOD => self.progress(&params)?,
            lsp::notification::LogMessage::METHOD => self.window_log_message(&params)?,
            lsp::notification::ShowMessage::METHOD => self.window_show_message(&params)?,
            lsp::notification::Exit::METHOD => self.exit(&params)?,
            // Extensions.
            NOTIFICATION__HandleFileType => self.handle_file_type(&params)?,
            NOTIFICATION__HandleBufNewFile => self.handle_buf_new_file(&params)?,
            NOTIFICATION__HandleBufEnter => self.handle_buf_enter(&params)?,
            NOTIFICATION__HandleTextChanged => self.handle_text_changed(&params)?,
            NOTIFICATION__HandleBufWritePost => self.handle_buf_write_post(&params)?,
            NOTIFICATION__HandleBufDelete => self.handle_buf_delete(&params)?,
            NOTIFICATION__HandleCursorMoved => self.handle_cursor_moved(&params)?,
            NOTIFICATION__HandleCompleteDone => self.handle_complete_done(&params)?,
            NOTIFICATION__FZFSinkLocation => self.fzf_sink_location(&params)?,
            NOTIFICATION__FZFSinkCommand => self.fzf_sink_command(&params)?,
            NOTIFICATION__ClearDocumentHighlight => self.clear_document_highlight(&params)?,
            NOTIFICATION__LanguageStatus => self.language_status(&params)?,
            NOTIFICATION__WindowProgress => self.window_progress(&params)?,
            NOTIFICATION__ServerExited => self.handle_server_exited(&params)?,

            _ => {
                let languageId_target = if languageId.is_some() {
                    // Message from language server. No handler found.
                    let msg = format!("Message not handled: {:?}", notification);
                    if notification.method.starts_with('$') {
                        warn!("{}", msg);
                        return Ok(());
                    } else {
                        return Err(err_msg(msg));
                    }
                } else {
                    // Message from vim. Proxy to language server.
                    let filename = self.vim()?.get_filename(&params)?;
                    let languageId_target = self.vim()?.get_language_id(&filename, &params)?;
                    info!(
                        "Proxy message directly to language server: {:?}",
                        notification
                    );
                    Some(languageId_target)
                };

                self.get_client(&languageId_target)?
                    .notify(&notification.method, &params)?;
            }
        };

        Ok(())
    }
}
