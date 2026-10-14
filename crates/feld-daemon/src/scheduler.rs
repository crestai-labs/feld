use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, watch};

use crate::control::{ScheduleTickResult, SessionController};
use crate::store::{ScheduledActor, Store};
use feld_core::model::{Schedule, next_run_after};
use feld_core::time;

const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const PLANNING_ERROR_RETRY_SECONDS: i64 = 30;
/// The longest the Scheduler sleeps before it reads the wall clock again, so that it wakes
/// soon after the wall clock jumps past its tick, as it does across a sleep of the host.
const WALL_CLOCK_RECHECK: Duration = Duration::from_secs(30);
/// How far past its tick the Scheduler may wake and still fire it. A tick it wakes for any
/// later, as after a sleep of the host, is recorded as missed. The supervisor gives a
/// Scheduler that has not woken as long before it starts it again.
const LATE_AFTER: chrono::Duration = chrono::Duration::minutes(2);
/// Why the Scheduler records as missed a tick it reached too late, after a sleep of the
/// host or after retrying a failed tick.
const LATE_REASON: &str = "the Scheduler reached it more than 2 minutes late";
/// How the supervisor watches the Scheduler: how often it checks, how long past the tick
/// it waits for the Scheduler may stay without waking before it is started again, and how
/// long it waits before starting it again. The wait doubles, up to `restart_max`, for each
/// Scheduler that stops within `healthy_after` of its start.
#[derive(Clone, Copy, Debug)]
pub struct Watch {
    pub interval: Duration,
    pub stall_after: chrono::Duration,
    pub restart_after: Duration,
    pub restart_max: Duration,
    pub healthy_after: Duration,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(15),
            stall_after: LATE_AFTER,
            restart_after: Duration::from_secs(1),
            restart_max: Duration::from_secs(60),
            healthy_after: Duration::from_secs(60),
        }
    }
}

/// What the Scheduler is doing, as `feld daemon status` shows it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerStatus {
    /// When the Scheduler last woke, to plan or to fire a tick.
    pub woke_at: Option<String>,
    /// The tick each enabled Schedule waits for, earliest first.
    pub planned: Vec<PlannedTickStatus>,
    /// How many times the Scheduler was started again since the daemon started.
    pub restarts: u64,
    pub last_restart: Option<SchedulerRestart>,
    /// The instant the Scheduler sleeps until, while it sleeps for a tick.
    #[serde(skip)]
    waiting_until: Option<DateTime<Utc>>,
}

impl SchedulerStatus {
    /// Records that the Scheduler woke at `now`. It is awake until it goes to sleep again,
    /// however long it takes over the ticks it woke for.
    fn woke(&mut self, now: DateTime<Utc>) {
        self.woke_at = Some(time::format(now));
        self.waiting_until = None;
    }

    /// Whether the Scheduler has slept past its tick, and past the last jump of the wall
    /// clock, for longer than `stall_after`.
    fn stalled(
        &self,
        now: DateTime<Utc>,
        resumed: DateTime<Utc>,
        stall_after: chrono::Duration,
    ) -> bool {
        self.waiting_until
            .is_some_and(|deadline| now - deadline.max(resumed) > stall_after)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedTickStatus {
    pub workspace: String,
    pub actor: String,
    pub planned: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerRestart {
    pub at: String,
    pub reason: String,
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;

    fn sleep_until(&self, deadline: DateTime<Utc>) -> impl Future<Output = ()> + Send;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    async fn sleep_until(&self, deadline: DateTime<Utc>) {
        // tokio's clock stands still while the host sleeps, so the wait is cut into pieces
        // and measured again against the wall clock after each.
        while let Ok(delay) = (deadline - Utc::now()).to_std() {
            if delay.is_zero() {
                break;
            }
            tokio::time::sleep(delay.min(WALL_CLOCK_RECHECK)).await;
        }
    }
}

#[derive(Clone, Debug)]
struct PlannedTick {
    target: ScheduledActor,
    planned: String,
    instant: DateTime<Utc>,
    ready_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct FailedPlan {
    target: ScheduledActor,
    ready_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Scheduler {
    store: Arc<Store>,
    controller: Arc<SessionController>,
    reconfigured: Arc<Notify>,
    status: Arc<Mutex<SchedulerStatus>>,
}

impl Scheduler {
    pub fn new(
        store: Arc<Store>,
        controller: Arc<SessionController>,
        reconfigured: Arc<Notify>,
    ) -> Self {
        Self {
            store,
            controller,
            reconfigured,
            status: Arc::default(),
        }
    }

    pub fn status(&self) -> Arc<Mutex<SchedulerStatus>> {
        Arc::clone(&self.status)
    }

    /// Runs the Scheduler until shutdown. A Scheduler that ends, or that sleeps past the
    /// tick it waits for without waking, is logged and started again.
    pub async fn supervise(self, shutdown: watch::Receiver<bool>) {
        self.supervise_with(
            shutdown,
            Watch::default(),
            SystemClock,
            |scheduler, shutdown| scheduler.run(shutdown),
        )
        .await;
    }

    /// [`Scheduler::supervise`] with the watch, the wall clock and the run it supervises
    /// given.
    pub async fn supervise_with<C, F, R>(
        self,
        mut shutdown: watch::Receiver<bool>,
        watch: Watch,
        clock: C,
        run: R,
    ) where
        C: Clock,
        R: Fn(Self, watch::Receiver<bool>) -> F,
        F: Future<Output = ()> + Send + 'static,
    {
        let mut delay = watch.restart_after;
        loop {
            let started = tokio::time::Instant::now();
            let mut task = tokio::spawn(run(self.clone(), shutdown.clone()));
            let mut looked = (clock.now(), tokio::time::Instant::now());
            // The wall-clock time from which a Scheduler that has not woken counts as stalled:
            // a wall clock that runs ahead of the elapsed time (the host slept) gives the
            // Scheduler `stall_after` from the jump to notice it.
            let mut resumed = looked.0;
            let reason = loop {
                tokio::select! {
                    joined = &mut task => {
                        if *shutdown.borrow() {
                            return;
                        }
                        break match joined {
                            Ok(()) => "it ended".to_owned(),
                            Err(error) => format!("it failed: {}", join_error_message(error)),
                        };
                    }
                    _ = tokio::time::sleep(watch.interval) => {
                        let now = (clock.now(), tokio::time::Instant::now());
                        let elapsed = chrono::Duration::from_std(now.1 - looked.1)
                            .unwrap_or(chrono::Duration::MAX);
                        let jump = chrono::Duration::from_std(watch.interval)
                            .unwrap_or(chrono::Duration::MAX);
                        if now.0 - looked.0 - elapsed > jump {
                            resumed = now.0;
                        }
                        looked = now;
                        if self.stalled(now.0, resumed, watch.stall_after) {
                            task.abort();
                            let _ = (&mut task).await;
                            break "it has not woken for its tick".to_owned();
                        }
                    }
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            // A Scheduler stuck short of its next wake-up would never see
                            // the shutdown, so it is ended where it waits.
                            task.abort();
                            let _ = task.await;
                            return;
                        }
                    }
                }
            };
            if started.elapsed() >= watch.healthy_after {
                delay = watch.restart_after;
            }
            eprintln!("scheduler stopped: {reason}; restarting in {delay:?}");
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
            }
            self.record_restart(clock.now(), reason);
            delay = (delay * 2).min(watch.restart_max);
        }
    }

    /// Counts the restart and records as missed every tick from the one each Schedule
    /// waited for up to `now`, since the Scheduler it starts plans from now. A Schedule the
    /// stalled Scheduler did not plan, as one enabled or resumed after it last woke, waited
    /// for its first tick after it last woke. A Schedule enabled or changed after the
    /// Scheduler last woke waited for its first tick after that, as the ticks planned before
    /// follow the cron it had then.
    fn record_restart(&self, now: DateTime<Utc>, reason: String) {
        let (waited, woke_at) = {
            let mut status = self.status.lock().expect("Scheduler status lock poisoned");
            status.restarts += 1;
            status.last_restart = Some(SchedulerRestart {
                at: time::format(now),
                reason: reason.clone(),
            });
            status.waiting_until = None;
            (status.planned.clone(), status.woke_at.clone())
        };
        let woke_at = woke_at.and_then(|at| time::parse(&at).ok());
        let schedules = match self.store.list_enabled_schedules() {
            Ok(schedules) => schedules,
            Err(error) => {
                eprintln!("missed ticks were not recorded: {error:#}");
                return;
            }
        };
        for target in schedules {
            let waited = waited
                .iter()
                .find(|tick| tick.workspace == target.workspace && tick.actor == target.actor)
                .and_then(|tick| {
                    time::parse(&tick.planned)
                        .ok()
                        .map(|instant| (tick.planned.clone(), instant))
                });
            let since = [
                target.schedule.enabled_at.as_deref(),
                Some(&target.updated_at),
            ]
            .into_iter()
            .flatten()
            .filter_map(|at| time::parse(at).ok())
            .max();
            let mut next = waited.or_else(|| {
                woke_at.and_then(|woke_at| planned_after(&target.schedule, woke_at).ok())
            });
            if let Some(since) = since
                && (woke_at.is_some_and(|woke_at| woke_at < since)
                    || next.as_ref().is_some_and(|(_, instant)| *instant <= since))
            {
                next = planned_after(&target.schedule, since).ok();
            }
            while let Some((planned, instant)) = next {
                if instant > now {
                    break;
                }
                if let Err(error) = self.store.record_missed_tick(
                    &target.workspace,
                    &target.actor,
                    &planned,
                    &reason,
                ) {
                    eprintln!(
                        "missed tick of {}/{} at {planned} was not recorded: {error:#}",
                        target.workspace, target.actor
                    );
                }
                next = planned_after(&target.schedule, instant).ok();
            }
        }
    }

    fn stalled(
        &self,
        now: DateTime<Utc>,
        resumed: DateTime<Utc>,
        stall_after: chrono::Duration,
    ) -> bool {
        self.status
            .lock()
            .expect("Scheduler status lock poisoned")
            .stalled(now, resumed, stall_after)
    }

    fn woke(&self, now: DateTime<Utc>) {
        self.status
            .lock()
            .expect("Scheduler status lock poisoned")
            .woke(now);
    }

    /// Publishes the ticks the Scheduler waits for as it goes to sleep until `deadline`.
    fn publish(&self, plans: &[PlannedTick], deadline: Option<DateTime<Utc>>) {
        let mut planned: Vec<&PlannedTick> = plans.iter().collect();
        planned.sort_by_key(|plan| plan.instant);
        let mut status = self.status.lock().expect("Scheduler status lock poisoned");
        status.planned = planned
            .into_iter()
            .map(|plan| PlannedTickStatus {
                workspace: plan.target.workspace.clone(),
                actor: plan.target.actor.clone(),
                planned: plan.planned.clone(),
            })
            .collect();
        status.waiting_until = deadline;
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        loop {
            match self.run_with_clock(&SystemClock, shutdown.clone()).await {
                Ok(()) => return,
                Err(error) => eprintln!("scheduler failed: {error:#}; retrying"),
            }
            tokio::select! {
                _ = tokio::time::sleep(ERROR_RETRY_INTERVAL) => {}
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
                _ = self.reconfigured.notified() => {}
            }
        }
    }

    pub async fn run_with_clock<C: Clock>(
        &self,
        clock: &C,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        self.woke(clock.now());
        let (mut plans, mut failed_plans) = self.plans_after(clock.now())?;
        loop {
            if *shutdown.borrow() {
                return Ok(());
            }
            let notified = self.reconfigured.notified();
            let deadline = plans
                .iter()
                .map(|plan| plan.ready_at)
                .chain(failed_plans.iter().map(|plan| plan.ready_at))
                .min();
            self.publish(&plans, deadline);
            if let Some(deadline) = deadline {
                tokio::select! {
                    _ = clock.sleep_until(deadline) => {}
                    _ = notified => {
                        self.woke(clock.now());
                        self.reconcile_plans(clock.now(), &mut plans, &mut failed_plans)?;
                        continue;
                    }
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            return Ok(());
                        }
                        continue;
                    }
                }
            } else {
                tokio::select! {
                    _ = notified => {
                        self.woke(clock.now());
                        self.reconcile_plans(clock.now(), &mut plans, &mut failed_plans)?;
                        continue;
                    }
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            return Ok(());
                        }
                        continue;
                    }
                }
            }

            let now = clock.now();
            self.woke(now);
            let mut due = Vec::new();
            plans.retain(|plan| {
                if plan.ready_at <= now {
                    due.push(plan.clone());
                    false
                } else {
                    true
                }
            });
            let mut retry_plans = Vec::new();
            failed_plans.retain(|plan| {
                if plan.ready_at <= now {
                    retry_plans.push(plan.clone());
                    false
                } else {
                    true
                }
            });
            for failed in retry_plans {
                match plan_tick(failed.target.clone(), now) {
                    Ok(plan) => plans.push(plan),
                    Err(error) => {
                        eprintln!(
                            "schedule planning failed for {}/{}: {error:#}; retrying",
                            failed.target.workspace, failed.target.actor
                        );
                        failed_plans.push(FailedPlan {
                            target: failed.target,
                            ready_at: now + chrono::Duration::seconds(PLANNING_ERROR_RETRY_SECONDS),
                        });
                    }
                }
            }
            due.sort_by(|left, right| {
                left.instant
                    .cmp(&right.instant)
                    .then_with(|| left.target.workspace.cmp(&right.target.workspace))
                    .then_with(|| left.target.actor.cmp(&right.target.actor))
            });
            let mut reload = false;
            for tick in due {
                if *shutdown.borrow() {
                    return Ok(());
                }
                let now = clock.now();
                if now - tick.instant > LATE_AFTER {
                    match self.miss_late_ticks(&tick, now) {
                        Ok(true) => plans.push(plan_tick(tick.target, now)?),
                        Ok(false) => reload = true,
                        Err(error) => {
                            eprintln!(
                                "missed ticks of {}/{} from {} were not recorded: {error:#}; retrying",
                                tick.target.workspace, tick.target.actor, tick.planned
                            );
                            plans.push(PlannedTick {
                                ready_at: now + chrono::Duration::seconds(1),
                                ..tick
                            });
                        }
                    }
                    continue;
                }
                let accepted = time::format(now);
                let result = self
                    .controller
                    .schedule_tick(
                        &tick.target.workspace,
                        &tick.target.actor,
                        &tick.target.schedule,
                        &tick.planned,
                        &accepted,
                        &shutdown,
                    )
                    .await;
                match result {
                    Ok(
                        ScheduleTickResult::Started(_)
                        | ScheduleTickResult::Skipped(_)
                        | ScheduleTickResult::Already,
                    ) => {
                        plans.push(plan_tick(tick.target, tick.instant)?);
                    }
                    Ok(ScheduleTickResult::Stale) => reload = true,
                    Ok(ScheduleTickResult::Cancelled) => return Ok(()),
                    Err(error) => {
                        eprintln!(
                            "scheduled tick failed for {}/{} at {}: {error:#}; retrying",
                            tick.target.workspace, tick.target.actor, tick.planned
                        );
                        plans.push(PlannedTick {
                            ready_at: clock.now() + chrono::Duration::seconds(1),
                            ..tick
                        });
                    }
                }
            }
            if reload {
                self.reconcile_plans(clock.now(), &mut plans, &mut failed_plans)?;
            }
        }
    }

    /// Records as missed `tick`, which the Scheduler reached too late, and every later tick
    /// up to `now`. Returns false, recording nothing, when the Schedule changed since `tick`
    /// was planned.
    fn miss_late_ticks(&self, tick: &PlannedTick, now: DateTime<Utc>) -> Result<bool> {
        let target = &tick.target;
        let mut planned = Vec::new();
        let mut next = Some((tick.planned.clone(), tick.instant));
        while let Some((at, instant)) = next.take().filter(|(_, instant)| *instant <= now) {
            planned.push(at);
            next = planned_after(&target.schedule, instant).ok();
        }
        self.store.record_missed_ticks_of(
            &target.workspace,
            &target.actor,
            &target.schedule,
            &planned,
            LATE_REASON,
        )
    }

    fn plans_after(&self, after: DateTime<Utc>) -> Result<(Vec<PlannedTick>, Vec<FailedPlan>)> {
        let mut plans = Vec::new();
        let mut failures = Vec::new();
        for target in self.store.list_enabled_schedules()? {
            match plan_tick(target.clone(), after) {
                Ok(plan) => plans.push(plan),
                Err(error) => {
                    eprintln!(
                        "schedule planning failed for {}/{}: {error:#}; retrying",
                        target.workspace, target.actor
                    );
                    failures.push(FailedPlan {
                        target,
                        ready_at: after + chrono::Duration::seconds(PLANNING_ERROR_RETRY_SECONDS),
                    });
                }
            }
        }
        Ok((plans, failures))
    }

    fn reconcile_plans(
        &self,
        after: DateTime<Utc>,
        plans: &mut Vec<PlannedTick>,
        failures: &mut Vec<FailedPlan>,
    ) -> Result<()> {
        let targets = self.store.list_enabled_schedules()?;
        let is_current = |target: &ScheduledActor| {
            targets.iter().any(|current| {
                current.workspace == target.workspace
                    && current.actor == target.actor
                    && current.schedule.revision == target.schedule.revision
            })
        };
        plans.retain(|plan| is_current(&plan.target));
        failures.retain(|failure| is_current(&failure.target));

        for target in targets {
            let exists = plans.iter().any(|plan| {
                plan.target.workspace == target.workspace && plan.target.actor == target.actor
            }) || failures.iter().any(|failure| {
                failure.target.workspace == target.workspace && failure.target.actor == target.actor
            });
            if exists {
                continue;
            }
            match plan_tick(target.clone(), after) {
                Ok(plan) => plans.push(plan),
                Err(error) => {
                    eprintln!(
                        "schedule planning failed for {}/{}: {error:#}; retrying",
                        target.workspace, target.actor
                    );
                    failures.push(FailedPlan {
                        target,
                        ready_at: after + chrono::Duration::seconds(PLANNING_ERROR_RETRY_SECONDS),
                    });
                }
            }
        }
        Ok(())
    }
}

fn join_error_message(error: tokio::task::JoinError) -> String {
    if !error.is_panic() {
        return error.to_string();
    }
    let panic = error.into_panic();
    panic
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panicked".to_owned())
}

fn plan_tick(target: ScheduledActor, after: DateTime<Utc>) -> Result<PlannedTick> {
    let (planned, instant) = planned_after(&target.schedule, after)?;
    Ok(PlannedTick {
        target,
        planned,
        instant,
        ready_at: instant,
    })
}

pub fn planned_after(schedule: &Schedule, after: DateTime<Utc>) -> Result<(String, DateTime<Utc>)> {
    let planned =
        next_run_after(&schedule.cron, &schedule.timezone, after).map_err(anyhow::Error::msg)?;
    let instant = time::parse(&planned).context("parse planned Schedule tick")?;
    Ok((planned, instant))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(instant: &str) -> DateTime<Utc> {
        instant.parse().unwrap()
    }

    #[test]
    fn a_scheduler_counts_as_stalled_only_while_it_sleeps_past_its_tick() {
        let stall_after = LATE_AFTER;
        let started = at("2026-09-16T00:00:30Z");
        let mut status = SchedulerStatus {
            waiting_until: Some(at("2026-09-16T00:01:00Z")),
            ..SchedulerStatus::default()
        };
        // Asleep past its tick for longer than `stall_after`.
        assert!(status.stalled(at("2026-09-16T00:03:01Z"), started, stall_after));

        // Awake for the tick, it may take longer than `stall_after` over it.
        status.woke(at("2026-09-16T00:01:00Z"));
        assert_eq!(status.woke_at.as_deref(), Some("2026-09-16T00:01:00.000Z"));
        assert!(!status.stalled(at("2026-09-16T00:10:00Z"), started, stall_after));
    }

    #[tokio::test]
    async fn a_scheduler_busy_over_its_tick_is_not_started_again() {
        let temporary = tempfile::tempdir().unwrap();
        let home =
            feld_core::home::FeldHome::resolve(Some(temporary.path().to_path_buf())).unwrap();
        home.create().unwrap();
        let store = Arc::new(Store::open(&home.store_path()).unwrap());
        let controller = Arc::new(SessionController::new(
            Arc::clone(&store),
            home,
            "http://127.0.0.1:1".to_owned(),
        ));
        let scheduler = Scheduler::new(store, controller, Arc::default());
        let status = scheduler.status();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        // The Scheduler woke for its tick and stays on it.
        let supervisor = tokio::spawn(scheduler.supervise_with(
            shutdown_rx,
            Watch {
                interval: Duration::from_millis(10),
                stall_after: chrono::Duration::zero(),
                restart_after: Duration::from_millis(10),
                ..Watch::default()
            },
            SystemClock,
            move |scheduler, _| async move {
                let tick = Utc::now();
                scheduler.publish(&[], Some(tick));
                scheduler.woke(tick);
                std::future::pending().await
            },
        ));

        while status.lock().unwrap().woke_at.is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(status.lock().unwrap().restarts, 0);

        shutdown_tx.send(true).unwrap();
        supervisor.await.unwrap();
    }
}
