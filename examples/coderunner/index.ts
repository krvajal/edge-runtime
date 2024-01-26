import { serve } from "https://deno.land/std@0.131.0/http/server.ts";

console.log("main function started");

type reqPayload = {
  script: string;
  args: Record<string, any>;
};

serve(async (req: Request) => {
  const url = new URL(req.url);
  const { pathname } = url;

  // handle health checks
  if (pathname === "/_internal/health") {
    return new Response(JSON.stringify({ message: "ok" }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }

  const { script }: reqPayload = await req.json();

  const path_parts = pathname.split("/");
  const service_name = path_parts[1];

  if (!service_name || service_name === "") {
    const error = { msg: "missing function name in request" };
    return new Response(JSON.stringify(error), {
      status: 400,
      headers: { "Content-Type": "application/json" },
    });
  }

  const servicePath = `./examples/${service_name}`;
  console.error(`serving the request with ${servicePath}`);

  const createWorker = async () => {
    const memoryLimitMb = 150;
    const workerTimeoutMs = 5 * 60 * 1000;
    const noModuleCache = false;
    // you can provide an import map inline
    const inlineImportMap = {
      imports: {
        "std/": "https://deno.land/std@0.131.0/",
        cors: "./examples/_shared/cors.ts",
        maker: "./examples/_shared/maker.js",
      },
    };
    // const importMapPath = `data:${encodeURIComponent(JSON.stringify(importMap))}?${encodeURIComponent('/home/deno/functions/test')}`;
    const importMapPath = null;
    const envVarsObj = Deno.env.toObject();
    const envVars = Object.keys(envVarsObj).map((k) => [k, envVarsObj[k]]);
    const forceCreate = false;
    const netAccessDisabled = false;

    // load source from an eszip
    //const maybeEszip = await Deno.readFile('./bin.eszip');
    //const maybeEntrypoint = 'file:///src/index.ts';

    // const maybeEntrypoint = 'file:///src/index.ts';
    // or load module source from an inline module
    const maybeModuleCode = createCode(script);
    //
    const cpuTimeSoftLimitMs = 50;
    const cpuTimeHardLimitMs = 100;

    return await EdgeRuntime.userWorkers.create({
      servicePath,
      memoryLimitMb,
      workerTimeoutMs,
      noModuleCache,
      importMapPath,
      inlineImportMap,
      envVars,
      forceCreate,
      netAccessDisabled,
      cpuTimeSoftLimitMs,
      cpuTimeHardLimitMs,
      // maybeEszip,
      // maybeEntrypoint,
      maybeModuleCode,
    });
  };

  const callWorker = async () => {
    try {
      // If a worker for the given service path already exists,
      // it will be reused by default.
      // Update forceCreate option in createWorker to force create a new worker for each request.
      const worker = await createWorker();
      const controller = new AbortController();

      const signal = controller.signal;
      // Optional: abort the request after a timeout
      //setTimeout(() => controller.abort(), 2 * 60 * 1000);

      return await worker.fetch(req, { signal });
    } catch (e) {
      console.error(e);

      if (e instanceof Deno.errors.WorkerRequestCancelled) {
        // XXX(Nyannyacha): I can't think right now how to re-poll
        // inside the worker pool without exposing the error to the
        // surface.

        // It is satisfied when the supervisor that handled the original
        // request terminated due to reaches such as CPU time limit or
        // Wall-clock limit.
        //
        // The current request to the worker has been canceled due to
        // some internal reasons. We should repoll the worker and call
        // `fetch` again.
        // return await callWorker();
        console.log("cancelled!");
      }

      const error = { msg: e.toString() };
      return new Response(JSON.stringify(error), {
        status: 500,
        headers: { "Content-Type": "application/json" },
      });
    }
  };

  return callWorker();
});

function createCode(src: string) {
  return `
		import __ChatlayerResponseBuilder from 'maker';
		import lodash from "npm:lodash@4.17.21";
		let results = [];
		const ChatlayerResponseBuilder = (...args) => {
		  const service = __ChatlayerResponseBuilder(...args);
		  service.send = () => {
			// get the current state as json
			results.push(service.toJSON())
		  };
		  return service;
		};
		console.log(results)

		async function main(args) {
			${src}
		}

  		Deno.serve(async (req) => {
			const {args} = await req.json();
			await main(args)
			return new Response(
				JSON.stringify({results, status: "ok"}),
				{ headers: { 'Content-Type': 'application/json', 'Connection': 'keep-alive' } },
			);
		})
	`;
}
