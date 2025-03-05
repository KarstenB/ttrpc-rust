// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use async_trait::async_trait;
use log::{error, trace};
use tokio::io::split;
use tokio::{io::ReadHalf, select, task};

use crate::error::Error;
use crate::proto::{GenMessage, GenMessageError, MessageHeader};

use super::{stream::SendingMessage, transport::Socket};

pub trait Builder {
    type Reader;
    type Writer;

    fn build(&mut self) -> (Self::Reader, Self::Writer);
}

#[async_trait]
pub trait WriterDelegate {
    async fn recv(&mut self) -> Option<SendingMessage>;
    async fn disconnect(&self, msg: &GenMessage, e: Error);
    async fn exit(&self);
}

#[async_trait]
pub trait ReaderDelegate {
    async fn wait_shutdown(&self);
    async fn disconnect(&self, e: Error, task: &mut task::JoinHandle<()>);
    async fn exit(&self);
    async fn handle_msg(&self, msg: GenMessage);
    async fn handle_err(&self, header: MessageHeader, e: Error);
}

pub struct Connection<B: Builder> {
    reader: ReadHalf<Socket>,
    writer_task: task::JoinHandle<()>,
    reader_delegate: B::Reader,
}

impl<B> Connection<B>
where
    B: Builder,
    B::Reader: ReaderDelegate + Send + Sync + 'static,
    B::Writer: WriterDelegate + Send + Sync + 'static,
{
    pub fn new(conn: Socket, mut builder: B) -> Self {
        let (reader, mut writer) = split(conn);

        let (reader_delegate, mut writer_delegate) = builder.build();

        // Long-running sender task
        let writer_task = tokio::spawn(async move {
            while let Some(mut sending_msg) = writer_delegate.recv().await {
                trace!("write message: {:?}", sending_msg.msg);
                if let Err(e) = sending_msg.msg.write_to(&mut writer).await {
                    error!("write_message got error: {:?}", e);
                    sending_msg.send_result(Err(e.clone()));
                    writer_delegate.disconnect(&sending_msg.msg, e).await;
                }
                sending_msg.send_result(Ok(()));
            }
            writer_delegate.exit().await;
            trace!("Writer task exit.");
        });

        Self {
            reader,
            writer_task,
            reader_delegate,
        }
    }

    pub async fn run(self) -> std::io::Result<()> {
        let Connection {
            mut reader,
            mut writer_task,
            reader_delegate,
        } = self;
        loop {
            select! {
                res = GenMessage::read_from(&mut reader) => {
                    match res {
                        Ok(msg) => {
                            trace!("Got Message {:?}", msg);
                            reader_delegate.handle_msg(msg).await;
                        }
                        Err(GenMessageError::ReturnError(header, e)) => {
                            trace!("Read msg err (can be return): {:?}", e);
                            reader_delegate.handle_err(header, e).await;
                        }

                        Err(GenMessageError::InternalError(e)) => {
                            trace!("Read msg err: {:?}", e);
                            reader_delegate.disconnect(e, &mut writer_task).await;
                            break;
                        }
                    }
                }
                _v = reader_delegate.wait_shutdown() => {
                    trace!("Receive shutdown.");
                    break;
                }
            }
        }
        reader_delegate.exit().await;
        trace!("Reader task exit.");

        Ok(())
    }
}
