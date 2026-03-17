use rove_engine::injection_detector::InjectionDetector;

#[test]
fn test_injection_detector_integration() {
    // Create detector
    let detector = InjectionDetector::new().expect("Failed to create detector");

    // Test all required injection phrases from Requirements 27.2
    let injection_phrases = vec![
        "ignore previous instructions",
        "disregard all",
        "new system prompt",
        "act as",
        "you are now",
        "forget your",
        "override your",
        "jailbreak",
        "DAN",
        "developer mode",
    ];

    for phrase in injection_phrases {
        let text = format!("Some text with {} in it", phrase);

        // Requirement 27.1: Scan tool results
        let warning = detector.scan(&text);
        assert!(warning.is_some(), "Failed to detect: {}", phrase);

        // Requirement 27.3: Block result from reaching LLM
        let sanitized = detector.sanitize(&text);
        assert_eq!(
            sanitized, "[INJECTION DETECTED - Content blocked for safety]",
            "Failed to block: {}",
            phrase
        );
    }
}

#[test]
fn test_clean_content_passes_through() {
    let detector = InjectionDetector::new().expect("Failed to create detector");

    let clean_texts = [
        "This is a normal file content",
        "The user wants to act on this file", // "act" alone shouldn't trigger
        "You can now proceed with the task",  // "you" and "now" separately shouldn't trigger
        "The developer mode is enabled",      // Should trigger
    ];

    // First 3 should pass, last one should be blocked
    for (i, text) in clean_texts.iter().enumerate() {
        let sanitized = detector.sanitize(text);
        if i < 3 {
            assert_eq!(
                sanitized, *text,
                "Clean text was incorrectly blocked: {}",
                text
            );
        } else {
            assert_eq!(
                sanitized, "[INJECTION DETECTED - Content blocked for safety]",
                "Injection was not blocked: {}",
                text
            );
        }
    }
}

#[test]
fn test_case_insensitive_detection() {
    let detector = InjectionDetector::new().expect("Failed to create detector");

    let variations = vec![
        "IGNORE PREVIOUS INSTRUCTIONS",
        "Ignore Previous Instructions",
        "ignore previous instructions",
        "IgNoRe PrEvIoUs InStRuCtIoNs",
    ];

    for text in variations {
        let warning = detector.scan(text);
        assert!(
            warning.is_some(),
            "Failed to detect case variation: {}",
            text
        );
    }
}

#[test]
fn test_injection_warning_details() {
    let detector = InjectionDetector::new().expect("Failed to create detector");

    let text = "Normal text before ignore previous instructions and after";
    let warning = detector.scan(text).expect("Should detect injection");

    // Requirement 27.4: Log detection with sanitized content
    assert!(!warning.matched_pattern.is_empty());
    assert!(warning.position > 0);
    assert_eq!(warning.position, 19); // Position where "ignore" starts
}

#[test]
fn test_multiple_injections_detects_first() {
    let detector = InjectionDetector::new().expect("Failed to create detector");

    let text = "First ignore previous instructions then disregard all";
    let warning = detector.scan(text).expect("Should detect injection");

    // Should detect the first occurrence
    assert!(warning.matched_pattern.to_lowercase().contains("ignore"));
}

#[test]
fn test_dan_word_boundary() {
    let detector = InjectionDetector::new().expect("Failed to create detector");

    // Should detect "DAN" as a word
    let text1 = "Enable DAN mode";
    assert!(detector.scan(text1).is_some());

    // Should not detect "DAN" as part of another word
    let text2 = "The dance was beautiful";
    assert!(detector.scan(text2).is_none());

    let text3 = "Abundant resources";
    assert!(detector.scan(text3).is_none());
}
