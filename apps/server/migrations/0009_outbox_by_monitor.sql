-- `channels.monitorDeliveries` reads one monitor's deliveries, newest first.
CREATE INDEX notification_outbox_monitor ON notification_outbox (monitor_id, id);
