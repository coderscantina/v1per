-- Monitors keep an order on their node, from 0, which `monitors.move` changes. Existing
-- monitors take the order lists showed so far: by name, then ID.
ALTER TABLE monitors ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

UPDATE monitors SET position = (
    SELECT COUNT(*) FROM monitors o
    WHERE o.node_id = monitors.node_id
      AND (o.name < monitors.name OR (o.name = monitors.name AND o.id < monitors.id))
);

DROP INDEX monitors_node;
CREATE INDEX monitors_node ON monitors (node_id, position);
