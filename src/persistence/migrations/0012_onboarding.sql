CREATE TABLE onboarding (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    step INTEGER NOT NULL CHECK(step BETWEEN 0 AND 3),
    completed INTEGER NOT NULL CHECK(completed IN (0, 1)),
    CHECK(completed = 0 OR step = 3)
);
INSERT INTO onboarding(id, step, completed) VALUES(1, 0, 0);
