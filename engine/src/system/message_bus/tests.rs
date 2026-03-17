use super::{Event, EventType, MessageBus};

#[tokio::test]
async fn test_subscribe_and_publish() {
    let bus = MessageBus::new();
    let mut rx = bus.subscribe(EventType::TaskStarted).await;

    bus.publish(Event::TaskStarted {
        task_id: "task-1".to_string(),
        input: "test input".to_string(),
    })
    .await;

    match rx.recv().await.unwrap() {
        Event::TaskStarted { task_id, input } => {
            assert_eq!(task_id, "task-1");
            assert_eq!(input, "test input");
        }
        _ => panic!("Wrong event type received"),
    }
}

#[tokio::test]
async fn test_multiple_subscribers() {
    let bus = MessageBus::new();
    let mut rx1 = bus.subscribe(EventType::TaskCompleted).await;
    let mut rx2 = bus.subscribe(EventType::TaskCompleted).await;

    bus.publish(Event::TaskCompleted {
        task_id: "task-2".to_string(),
        result: "success".to_string(),
    })
    .await;

    match (rx1.recv().await.unwrap(), rx2.recv().await.unwrap()) {
        (Event::TaskCompleted { task_id: id1, .. }, Event::TaskCompleted { task_id: id2, .. }) => {
            assert_eq!(id1, "task-2");
            assert_eq!(id2, "task-2");
        }
        _ => panic!("Wrong event types received"),
    }
}

#[tokio::test]
async fn test_all_event_type() {
    let bus = MessageBus::new();
    let mut rx_all = bus.subscribe(EventType::All).await;
    let mut rx_specific = bus.subscribe(EventType::TaskStarted).await;

    bus.publish(Event::TaskStarted {
        task_id: "task-3".to_string(),
        input: "test".to_string(),
    })
    .await;

    match (
        rx_all.recv().await.unwrap(),
        rx_specific.recv().await.unwrap(),
    ) {
        (Event::TaskStarted { task_id: id1, .. }, Event::TaskStarted { task_id: id2, .. }) => {
            assert_eq!(id1, "task-3");
            assert_eq!(id2, "task-3");
        }
        _ => panic!("Wrong event types received"),
    }
}

#[tokio::test]
async fn test_different_event_types() {
    let bus = MessageBus::new();
    let mut rx_started = bus.subscribe(EventType::TaskStarted).await;
    let mut rx_completed = bus.subscribe(EventType::TaskCompleted).await;

    bus.publish(Event::TaskStarted {
        task_id: "task-4".to_string(),
        input: "input".to_string(),
    })
    .await;

    bus.publish(Event::TaskCompleted {
        task_id: "task-5".to_string(),
        result: "result".to_string(),
    })
    .await;

    match rx_started.recv().await.unwrap() {
        Event::TaskStarted { task_id, .. } => assert_eq!(task_id, "task-4"),
        _ => panic!("Wrong event type"),
    }

    match rx_completed.recv().await.unwrap() {
        Event::TaskCompleted { task_id, .. } => assert_eq!(task_id, "task-5"),
        _ => panic!("Wrong event type"),
    }

    assert!(rx_started.try_recv().is_err());
    assert!(rx_completed.try_recv().is_err());
}
