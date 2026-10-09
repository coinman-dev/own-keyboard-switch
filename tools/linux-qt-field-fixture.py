#!/usr/bin/env python3
"""Qt widgets for isolated acceptance. Reports geometry and focus, never text."""
import json
import sys
from pathlib import Path
from PyQt6.QtCore import QPoint, QTimer, Qt
from PyQt6.QtWidgets import QApplication, QLineEdit, QPlainTextEdit, QVBoxLayout, QWidget

state, command = map(Path, sys.argv[1:3])
app = QApplication(sys.argv[:1])
main, other = QWidget(), QWidget()
main.setWindowTitle("OKBS Qt fields fixture")
other.setWindowTitle("OKBS Qt second fixture")
if "--frameless" in sys.argv:
    main.setWindowFlag(Qt.WindowType.FramelessWindowHint)
    other.setWindowFlag(Qt.WindowType.FramelessWindowHint)
main.resize(420, 300)
other.resize(420, 240)
layout = QVBoxLayout(main)
first, password, second = QPlainTextEdit(), QLineEdit(), QPlainTextEdit()
first.setPlainText("synthetic first document\nsecond line")
password.setText("synthetic password")
password.setEchoMode(QLineEdit.EchoMode.Password)
second.setPlainText("synthetic second document")
layout.addWidget(first)
layout.addWidget(password)
QVBoxLayout(other).addWidget(second)

def focus(widget, offset=4):
    widget.setFocus(Qt.FocusReason.OtherFocusReason)
    if isinstance(widget, QPlainTextEdit):
        cursor = widget.textCursor()
        cursor.setPosition(offset)
        widget.setTextCursor(cursor)

def snapshot():
    widget = app.focusWidget()
    name = "first" if widget is first else "password" if widget is password else "second" if widget is second else "none"
    value = {"focus": name}
    if widget in (first, second):
        window = widget.window()
        caret = widget.cursorRect()
        point = widget.viewport().mapTo(window, QPoint(caret.x(), caret.y() + widget.fontMetrics().height()))
        value.update(caret=[point.x(), point.y()], window_size=[window.width(), window.height()])
    temporary = state.with_suffix(".new")
    temporary.write_text(json.dumps(value))
    temporary.replace(state)

def poll():
    if command.exists():
        action = command.read_text().strip()
        command.unlink()
        if action == "password": focus(password)
        if action == "first": focus(first)
        if action == "second": focus(second)
        if action == "move-caret": focus(first, 14)
        if action == "show-second": other.show()
    snapshot()

main.show()
QTimer.singleShot(200, lambda: focus(first))
timer = QTimer()
timer.timeout.connect(poll)
timer.start(20)
sys.exit(app.exec())
