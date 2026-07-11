from app.polish import apply_rules


def test_removes_fillers():
    assert apply_rules("um hello uh world") == "Hello world."


def test_collapses_stutters():
    assert apply_rules("the the meeting is is at noon") == "The meeting is at noon."


def test_capitalizes_and_punctuates():
    assert apply_rules("hello world. how are you") == "Hello world. How are you."


def test_dictionary_casing():
    assert (
        apply_rules("tell whispr to email grigorov", ["Whispr", "Grigorov"])
        == "Tell Whispr to email Grigorov."
    )


def test_keeps_existing_punctuation():
    assert apply_rules("Is this working?") == "Is this working?"


def test_filler_only_utterance_is_empty():
    assert apply_rules("um uh hmm") == ""


def test_spacing_before_punctuation():
    assert apply_rules("hello , world .") == "Hello, world."


def test_polishes_russian_text_without_changing_language():
    assert apply_rules("привет мир. как дела") == "Привет мир. Как дела."
