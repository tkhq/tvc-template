/**
 * Trigger evaluation for TVC skills.
 * Tests whether a skill's description causes Claude to load it for the right queries.
 *
 * Usage:
 *   npx tsx skills/tvc-app-builder/scripts/eval-triggers.ts --skill tvc-app-builder
 *   npx tsx skills/tvc-app-builder/scripts/eval-triggers.ts --skill tvc-app-builder --runs 3
 *   npx tsx skills/tvc-app-builder/scripts/eval-triggers.ts --skill tvc-app-builder --threshold 90 --concurrency 4
 *
 * Adapted from Anthropic's skill-creator run_eval.py (Apache 2.0).
 */

import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import matter from "gray-matter";

interface TriggerTestCase {
  should_trigger: string[];
  should_not_trigger: string[];
}

interface EvalResult {
  query: string;
  expected: boolean;
  triggered: boolean;
  pass: boolean;
}

async function runWithConcurrency<T>(
  tasks: (() => Promise<T>)[],
  concurrency: number
): Promise<T[]> {
  const results: T[] = new Array(tasks.length);
  let index = 0;
  async function worker() {
    while (index < tasks.length) {
      const i = index++;
      results[i] = await tasks[i]();
    }
  }
  await Promise.all(
    Array.from({ length: Math.min(concurrency, tasks.length) }, () => worker())
  );
  return results;
}

function loadTriggers(skillName: string): TriggerTestCase {
  const triggersPath = path.join("skills", skillName, "evals", "triggers.json");
  if (!fs.existsSync(triggersPath)) {
    console.error(`No triggers.json found at ${triggersPath}`);
    process.exit(1);
  }
  return JSON.parse(fs.readFileSync(triggersPath, "utf-8"));
}

function getSkillDescription(skillName: string): string {
  const skillMdPath = path.join("skills", skillName, "SKILL.md");
  const content = fs.readFileSync(skillMdPath, "utf-8");
  const parsed = matter(content);
  return parsed.data.description || "";
}

function testTrigger(
  query: string,
  skillName: string,
  pluginDir: string
): Promise<boolean> {
  return new Promise((resolve) => {
    const proc = spawn(
      "claude",
      [
        "-p", query,
        "--plugin-dir", pluginDir,
        "--output-format", "stream-json",
        "--verbose",
      ],
      { stdio: ["pipe", "pipe", "pipe"] }
    );

    let found = false;
    let buffer = "";
    let stderrOutput = "";
    const timeout = setTimeout(() => {
      if (!found) {
        proc.kill("SIGTERM");
        resolve(false);
      }
    }, 30000);

    proc.stderr.on("data", (chunk: Buffer) => {
      stderrOutput += chunk.toString();
    });

    proc.stdout.on("data", (chunk: Buffer) => {
      if (found) return;
      buffer += chunk.toString();
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";

      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const event = JSON.parse(line);
          if (event.error === "authentication_failed" || event.is_error) {
            console.error(`Auth error for query "${query}": ${event.result || event.error}`);
            found = false;
            clearTimeout(timeout);
            proc.kill("SIGTERM");
            resolve(false);
            return;
          }
          if (event.type === "assistant" && event.message?.content) {
            const content = Array.isArray(event.message.content)
              ? event.message.content
              : [event.message.content];
            for (const block of content) {
              if (
                block.type === "tool_use" &&
                block.name === "Skill" &&
                typeof block.input?.skill === "string" &&
                block.input.skill.includes(skillName)
              ) {
                found = true;
                clearTimeout(timeout);
                proc.kill("SIGTERM");
                resolve(true);
                return;
              }
            }
          }
        } catch {
          // Skip non-JSON lines
        }
      }
    });

    proc.on("close", () => {
      if (!found) {
        clearTimeout(timeout);
        if (stderrOutput.trim()) {
          console.error(`stderr for query "${query}": ${stderrOutput.trim().substring(0, 200)}`);
        }
        resolve(false);
      }
    });

    proc.on("error", (err) => {
      if (!found) {
        clearTimeout(timeout);
        console.error(`Process error for query "${query}": ${err.message}`);
        resolve(false);
      }
    });
  });
}

function parseArgs(): { skill: string; runs: number; threshold: number; concurrency: number } {
  const args = process.argv.slice(2);
  let skill = "";
  let runs = 1;
  let threshold = 100;
  let concurrency = 1;

  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--skill" && args[i + 1]) {
      skill = args[++i];
    } else if (args[i] === "--runs" && args[i + 1]) {
      runs = parseInt(args[++i], 10);
    } else if (args[i] === "--threshold" && args[i + 1]) {
      threshold = parseFloat(args[++i]);
    } else if (args[i] === "--concurrency" && args[i + 1]) {
      concurrency = parseInt(args[++i], 10);
    }
  }

  if (!skill) {
    console.error("Usage: eval-triggers.ts --skill <skill-name> [--runs N] [--threshold N] [--concurrency N]");
    process.exit(1);
  }

  return { skill, runs, threshold, concurrency };
}

async function main() {
  const { skill, runs, threshold, concurrency } = parseArgs();
  const pluginDir = process.cwd();

  const skillDir = path.join("skills", skill);
  if (!fs.existsSync(path.join(skillDir, "SKILL.md"))) {
    console.error(`Skill not found: ${skillDir}`);
    process.exit(1);
  }

  const triggers = loadTriggers(skill);
  const description = getSkillDescription(skill);

  console.log(`Evaluating triggers for: ${skill}`);
  console.log(`Description (${description.length} chars): ${description.substring(0, 100)}...`);
  console.log(`Runs per query: ${runs}`);
  console.log(`Threshold: ${threshold}%`);
  console.log(`Concurrency: ${concurrency}`);
  console.log(`Should trigger: ${triggers.should_trigger.length} queries`);
  console.log(`Should not trigger: ${triggers.should_not_trigger.length} queries`);
  console.log("");

  interface QueryTask {
    query: string;
    expected: boolean;
  }

  const queryTasks: QueryTask[] = [
    ...triggers.should_trigger.map((q) => ({ query: q, expected: true })),
    ...triggers.should_not_trigger.map((q) => ({ query: q, expected: false })),
  ];

  const tasks = queryTasks.map((qt) => async (): Promise<EvalResult> => {
    let triggerCount = 0;
    for (let r = 0; r < runs; r++) {
      const triggered = await testTrigger(qt.query, skill, pluginDir);
      if (triggered) triggerCount++;
    }
    const triggered = triggerCount > runs / 2;
    const pass = qt.expected ? triggered : !triggered;
    const label = qt.expected ? "should trigger" : "should NOT trigger";
    console.log(`${pass ? "PASS" : "FAIL"}  [${label}]${qt.expected ? "     " : " "}"${qt.query}" (${triggerCount}/${runs})`);
    return { query: qt.query, expected: qt.expected, triggered, pass };
  });

  const results = await runWithConcurrency(tasks, concurrency);

  const passed = results.filter((r) => r.pass).length;
  const total = results.length;
  const accuracy = ((passed / total) * 100).toFixed(1);

  console.log(`\nResults: ${passed}/${total} passed (${accuracy}%)`);

  const shouldTriggerResults = results.filter((r) => r.expected);
  const shouldNotTriggerResults = results.filter((r) => !r.expected);
  const triggerAccuracy =
    shouldTriggerResults.length > 0
      ? ((shouldTriggerResults.filter((r) => r.pass).length / shouldTriggerResults.length) * 100).toFixed(1)
      : "N/A";
  const falsePositiveRate =
    shouldNotTriggerResults.length > 0
      ? ((shouldNotTriggerResults.filter((r) => !r.pass).length / shouldNotTriggerResults.length) * 100).toFixed(1)
      : "N/A";

  console.log(`Trigger accuracy: ${triggerAccuracy}%`);
  console.log(`False positive rate: ${falsePositiveRate}%`);
  console.log(`Threshold: ${threshold}%`);

  const accuracyNum = parseFloat(accuracy);
  const passed_threshold = accuracyNum >= threshold;
  console.log(`Status: ${passed_threshold ? "PASS" : "FAIL"}`);

  const outputPath = path.join(skillDir, "evals", "trigger-results.json");
  fs.writeFileSync(
    outputPath,
    JSON.stringify(
      {
        skill,
        timestamp: new Date().toISOString(),
        runs,
        threshold,
        results,
        summary: {
          total,
          passed,
          accuracy: accuracyNum,
          triggerAccuracy: parseFloat(triggerAccuracy as string) || 0,
          falsePositiveRate: parseFloat(falsePositiveRate as string) || 0,
        },
      },
      null,
      2
    )
  );
  console.log(`\nResults saved to ${outputPath}`);

  process.exit(passed_threshold ? 0 : 1);
}

main();
