//! Windows : place FS Code dans un Job Object « kill on close ».
//! Si l'app plante ou est tuée, Windows ferme le job et tue aussi `cloudflared`,
//! les serveurs MCP et les processus enfants (ils héritent du job).
//! Ailleurs : sans effet (le nettoyage se fait au prochain lancement).

#[cfg(windows)]
pub fn init() -> Result<(), String> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::System::JobObjects::{
 AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY : appels Win32 sur des structures locales ; le handle du job est volontairement
    // conservé ouvert jusqu'à la fin du processus (c'est sa fermeture qui déclenche le kill).
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
 if job.is_null() {
     return Err("CreateJobObjectW a échoué".to_string());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
     job,
     JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
     size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
     return Err("SetInformationJobObject a échoué".to_string());
        }
        if AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
     return Err("AssignProcessToJobObject a échoué (déjà dans un job ?)".to_string());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn init() -> Result<(), String> {
    Ok(())
}
