# The trailer's takes.
scene("today", [], [("wait", "", 2600)])
scene("capture", [], [
    ("key", "n", 600),
    ("type", "Algorithms exam friday at 9am for 2h in exams", 55),
    ("wait", "", 1800),
    ("key", "Enter", 900),
    ("key", "Escape", 300),
    ("key", "Escape", 1400),
])
scene("spanish", [], [
    ("key", "n", 500),
    ("type", "Llamar al dentista mañana a las 6 cada lunes", 55),
    ("wait", "", 2000),
])
scene("week", ["5", "@400"], [("wait", "", 1500), ("key", "v", 1600)])
scene("month", ["6", "@400"], [("wait", "", 1200), ("key", "j", 450), ("key", "j", 450), ("key", "j", 450), ("key", "j", 450), ("key", "j", 1200)])
scene("calendars", [",", "Enter", "j", "j", "j", "@4000"], [("wait", "", 2600)])
scene("themes", [], [("wait", "", 900),
    ("key", "T", 0), ("key", "j", 0), ("key", "Enter", 0), ("settle", "", 0), ("wait", "", 1100),
    ("key", "T", 0), ("key", "j", 0), ("key", "Enter", 0), ("settle", "", 0), ("wait", "", 1100),
    ("key", "T", 0), ("key", "j", 0), ("key", "Enter", 0), ("settle", "", 0), ("wait", "", 1100)])
