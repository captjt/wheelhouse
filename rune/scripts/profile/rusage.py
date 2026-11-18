"""A process's counters from the kernel (proc_pid_rusage, v4): CPU time,
physical footprint (Activity Monitor's Memory), wakeups, energy billed,
instructions. No extra permission for your own processes."""
import ctypes
import subprocess

_names = [
    "user_time", "system_time", "pkg_idle_wkups", "interrupt_wkups", "pageins", "wired_size", "resident_size",
    "phys_footprint", "proc_start_abstime", "proc_exit_abstime", "child_user_time", "child_system_time",
    "child_pkg_idle_wkups", "child_interrupt_wkups", "child_pageins", "child_elapsed_abstime", "diskio_bytesread",
    "diskio_byteswritten", "cpu_time_qos_default", "cpu_time_qos_maintenance", "cpu_time_qos_background",
    "cpu_time_qos_utility", "cpu_time_qos_legacy", "cpu_time_qos_user_initiated", "cpu_time_qos_user_interactive",
    "billed_system_time", "serviced_system_time", "logical_writes", "lifetime_max_phys_footprint", "instructions",
    "cycles", "billed_energy", "serviced_energy", "interval_max_phys_footprint", "runnable_time",
]


class _RU(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(n, ctypes.c_uint64) for n in _names]


class _TB(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


_libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
_tb = _TB()
_libc.mach_timebase_info(ctypes.byref(_tb))
TICK_NS = _tb.numer / _tb.denom


def sample(pid):
    ru = _RU()
    if _libc.proc_pid_rusage(int(pid), 4, ctypes.byref(ru)) != 0:
        return None
    return {
        "cpu_ns": (ru.user_time + ru.system_time) * TICK_NS,
        "footprint": ru.phys_footprint,
        "max_footprint": ru.lifetime_max_phys_footprint,
        "wakeups": ru.pkg_idle_wkups + ru.interrupt_wkups,
        "energy_nj": ru.billed_energy,
        "instructions": ru.instructions,
        "cycles": ru.cycles,
    }


def ps_cpu_s(pid):
    """CPU seconds from ps, for a process we can't read the counters of (WindowServer)."""
    out = subprocess.run(["ps", "-o", "time=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    if not out:
        return None
    parts = [float(x) for x in out.replace("-", ":").split(":")]
    s = 0.0
    for p in parts:
        s = s * 60 + p
    return s
