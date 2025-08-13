# Auto-batching proxy for inference requests

## Task Description

The task was to create an auto-batching proxy service that would serve as a wrapper
over another inference service (see `Makefile` for details on how we are launching that
service for development purposes). Internally, that batching proxy will 🥁 batch
individual embedding requests, while for the end-user the API is the same, as if
they were the only client of the inference service. This batching makes requests
to the upstream service more efficient (and helps reduce costs).

In my hometown in 1990-2000s, there used to be drivers hanging around the realway
station who - if you missed your train or just did not bother to buy a ticket, would
offer you a ride to another town - but a shared ride. They would gather (batch)
a few fellas like myself and then start the ride. But there were rules - they
could not take more that N people (depending on the vehicle size) and one person
who came first could not wait for too long (like no longer than an hour normally).

In the similar fashion, in our batching service here we got `MAX_BATCH_SIZE` and
`MAX_WAIT_TIME` (in millis) parameters configurable via the environment (see other configurable
options in `.env.example`).

## Solution

### Stack

Our REST API wrapper is powered by axum web-framework, which is our framework
of choice. We have not added Openapi definitions to this project, but if we need
to, we will integrate `utoipa` crate. All other crates we depend on are pretty
standard.

### Interface

Our REST API wrapper is powered by axum and currently provides one single
endpoint `POST /embed` and so trying it our is super straightforward:

```console
curl 127.0.0.1:8081/embed -X POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json'
```

Notice how it looks exactly the same (but for PORT number) if you were querying
the [upstream service][4] directly:

```console
curl 127.0.0.1:8080/embed -X POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json'
```

### Handler

On the app's start-up, we are launching a task with the web-server and a dedicated
task for our inference service worker - which is effectively an actor responsible
for inference and hiding the implementation details from the axum handler to separate
concerns. Instances of the handler (threads processing end-users' requests) are
communicating with that worker using channels and messages. Once a handler receives
a request it forwards it to the worker and awaits the worker's response (embeddings or
an error) via a oneshot channel, and once it gets the response, it sends it's
JSON representation to the end-user.

### Worker

The worker just listens for messages from the axum handlers. The worker keeps
some state: it has got a message queue with a capacity as per `MAX_BATCH_SIZE`
and a timeout as per `MAX_WAIT_TIME` - whichever comes first will make the worker
send the batch to the upstream service. If an error is received from the
upstream inference service, it gets "broadcast" to the handlers. If the embeddings
are received, the worker will make sure to not send the entirety of it to each
handler, rather only the segment that corresponds to the handler's inputs. We
are relying here on the fact that the upstream service returns an array of embeddings
in which an embdedding at index N is the result for the query at index N in the
inputs container in our request.

To give a concrete example, imagine the batch size is set to `2`, and the first
request contains inputs array `["hello", "world"]` while the second request has
only one item `["bye"]` - the worker will flatten these two into one array and
send to the upstream service as `["hello", "world", "bye"]`. The response our worker
gets will have the following shape:
`[[-0.045, ... , -0.123144], [0.412, ..., -0.412], [0.1241, ..., 0.123]]`.
The worker still "remembers" at that point that it needs to send `2` embeddings
to the first handler and `1` embeddig to the second handler instance.

Also - replaying the example above - if the batch size is set to `2`, the worker
received a message from one handler `["hello", "world"]` and the time-out
(configured via `MAX_WAIT_TIME`) is reached, the worker will send
send `["hello", "world"]` to the upstream server.

Each time the batch get "flushed", the timeout gets unset and the queue gets
emptied.

## Demo

NB: make sure you got [`GNU Make`][2], and [`docker`][3] installed.

Populate your very own local `.env` file with:

```console
make dotenv
```

You can now launch the auto-batching proxy together with the inference service
with a single command:

```console
docker compose up --build # same as `make compose/up`
```

The command above will build our proxy app, launch the upstream inference service
first, make sure it is ready, and then launch the proxy app. The initial image build
takes some time plus the model need some warm up, so the "cold" start can take
up to a few minutes.

If you tweak `MAX_WAIT_TIME` and `MAX_BATCH_SIZE` parameters in your `.env`
file, make sure to restart the containers.

## Benchmarking

We've set `MAX_WAIT_TIME` to `1000` (1 second) and `MAX_BATCH_SIZE` to `8`
(the upstream service's text embedding router batch cap), and `RUST_LOG`
set tot "auto_batching_proxy=error,axum=error".

We then launched the services as described [above](#demo) and used the [`oha`][5]
utility to generate some load.

### With proxy

The command used (see `load` target in [`Makefile`](./Makefile)):

```console
oha -c 200 -z 30s --latency-correction -m POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json' http://localhost:8081/embed
```

Which gave the following results:

```
  Success rate: 100.00%
  Total:        30.0039 sec
  Slowest:      2.0037 sec
  Fastest:      0.2129 sec
  Average:      1.6135 sec
  Requests/sec: 126.6503

  Total data:   62.68 MiB
  Size/request: 17.83 KiB
  Size/sec:     2.09 MiB
```

### Without proxy

We've used same utility on the same hardware and some max batch size and max wait,
but specified the upstream service's port in the command for direct communitation.
The command used (note the port number and see how we are mapping to this host port
in our [`compose`](./compose.yaml) and also take a look at `load/noproxy`
target in [`Makefile`](./Makefile))):

```console
oha -c 200 -z 30s --latency-correction -m POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json' http://localhost:8080/embed
```

```
  Success rate: 100.00%
  Total:        30.0047 sec
  Slowest:      2.1063 sec
  Fastest:      0.0452 sec
  Average:      1.6371 sec
  Requests/sec: 124.8803

  Total data:   64.19 MiB
  Size/request: 18.53 KiB
  Size/sec:     2.14 MiB
```

### Observations

The reports above are examples from one single test run. In general - upon a few
load test runs - we are observing pretty close request per second indicator.
Also the slowest requests are pretty close to each other, while the fastest request
without proxy is 2.5x faster (~30-100ms vs ~100-200ms), i.e. our wrapper _does_
introduce some overhead. Apparently, we are compensating for this with the gains
elsewhere - in the resources savings on the upstream service size and reduced costs
for each individual user.

Subscribing for debug and trace events and writing those to stdout slows our
application down (~20% bandwidth reduction), so we ended up testing with error+
events level.

We also tried loading our auto-batching proxy with `MAX_BATCH_SIZE` set to `1`
(and all other parameters the same), which gave us results close to those without
proxy. Here are stats from one of the runs:

```
  Success rate: 100.00%
  Total:        30.0061 sec
  Slowest:      2.0051 sec
  Fastest:      0.0672 sec
  Average:      1.5677 sec
  Requests/sec: 129.8068

  Total data:   65.14 MiB
  Size/request: 18.05 KiB
  Size/sec:     2.17 MiB
```

Which checks out: with the current implementation, the 8th client in the proxied
scenario with 8 messages per batch will wait till the preceding 7 clients get
their slices of the upstream inference service response. We could play around this
and try and improve implementation so reduce the proxy overhead.

## Dev Setup

Make sure you got [`cargo`][1], [`GNU Make`][2], and [`docker`][3] installed,
and hit:

```console
make setup
```

You should now be able to start the back-end in watch mode with:

```console
make watch
```

You can send requests with:

```console
curl 127.0.0.1:8081/embed -X POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json'
```

You can also tweak configurations in the generated `.env` file (gets populated
via `make setup`), the dev-server will restart automatically (if you are using
the `make watch` command as described above).

<!-- -------------------------------- LINKS -------------------------------- -->
[1]: https://doc.rust-lang.org/cargo/getting-started/installation.html
[2]: https://www.gnu.org/software/make/
[3]: https://docs.docker.com/engine/install/
[4]: https://github.com/huggingface/text-embeddings-inference
[5]: https://github.com/hatoo/oha?tab=readme-ov-file#installation
