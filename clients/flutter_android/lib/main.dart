import 'package:flutter/material.dart';
import 'bridge_generated.dart';
import 'dart:ffi';
import 'dart:io';
import 'package:http/http.dart' as http;

const _libName = 'xavier_mobile';
final DynamicLibrary _dylib = () {
  if (Platform.isAndroid || Platform.isLinux) {
    return DynamicLibrary.open('lib$_libName.so');
  }
  if (Platform.isMacOS || Platform.isIOS) {
    return DynamicLibrary.open('lib$_libName.dylib');
  }
  if (Platform.isWindows) {
    return DynamicLibrary.open('$_libName.dll');
  }
  throw UnsupportedError('Unknown platform: ${Platform.operatingSystem}');
}();

final api = XavierMobileImpl(_dylib);

void main() {
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Xavier Mobile',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepPurple),
        useMaterial3: true,
      ),
      home: const MyHomePage(title: 'Xavier Mobile Home Page'),
    );
  }
}

class MyHomePage extends StatefulWidget {
  const MyHomePage({super.key, required this.title});

  final String title;

  @override
  State<MyHomePage> createState() => _MyHomePageState();
}

class _MyHomePageState extends State<MyHomePage> {
  String _status = 'Initializing...';
  String _healthStatus = 'Unknown';

  @override
  void initState() {
    super.initState();
    _initXavier();
  }

  Future<void> _initXavier() async {
    try {
      await api.initXavier();
      setState(() {
        _status = 'Xavier Core Initialized';
      });

      // Start server in background
      api.startXavierServer(port: 8006).then((_) {
         print("Server exited");
      }).catchError((e) {
         print("Server error: $e");
      });

      setState(() {
        _status = 'Xavier Server Starting on 8006...';
      });

      // Wait a bit and check health
      Future.delayed(const Duration(seconds: 2), _checkHealth);

    } catch (e) {
      setState(() {
        _status = 'Error: $e';
      });
    }
  }

  Future<void> _checkHealth() async {
    try {
      final response = await http.get(Uri.parse('http://localhost:8006/health'));
      if (response.statusCode == 200) {
        setState(() {
          _healthStatus = 'Connected: ${response.body}';
          _status = 'Xavier Running';
        });
      } else {
        setState(() {
          _healthStatus = 'Error: ${response.statusCode}';
        });
      }
    } catch (e) {
      setState(() {
        _healthStatus = 'Error: $e';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        backgroundColor: Theme.of(context).colorScheme.inversePrimary,
        title: Text(widget.title),
      ),
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            const Text('Xavier Core Status:'),
            Text(
              _status,
              style: Theme.of(context).textTheme.headlineSmall,
            ),
            const SizedBox(height: 20),
            const Text('HTTP Health (localhost:8006):'),
            Text(
              _healthStatus,
              style: Theme.of(context).textTheme.bodyLarge,
            ),
            const SizedBox(height: 40),
            ElevatedButton(
              onPressed: _checkHealth,
              child: const Text('Refresh Health'),
            ),
          ],
        ),
      ),
    );
  }
}
