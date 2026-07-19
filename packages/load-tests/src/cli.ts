export {};

const command = process.argv[2] ?? "compute-server";

switch (command) {
	case "limits":
		await (await import("./local/limit-survival.js")).runLimitSurvival();
		break;
	case "limits-matrix":
		await (await import("./local/limit-matrix.js")).runLimitMatrix();
		break;
	case "scale":
		await (await import("./local/scale.js")).runScale();
		break;
	case "churn":
		await (await import("./local/churn-leak.js")).runChurnLeak();
		break;
	case "boundary":
		await (await import("./local/boundary.js")).runBoundary();
		break;
	case "compute-server":
		await import("./compute/server.js");
		break;
	case "compute-load":
		await (await import("./compute/controller.js")).runComputeLoad();
		break;
	default:
		throw new Error(
			`unknown load-test command ${command}; expected boundary, limits, limits-matrix, scale, churn, compute-server, or compute-load`,
		);
}
