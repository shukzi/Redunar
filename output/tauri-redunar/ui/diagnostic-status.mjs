export function diagnosticStatus(status, requested) {
 if(status==='unavailable')return 'Logging is unavailable. Check free space and state-folder permissions, then restart Redunar.';
 if(status==='active')return requested?'Logging is active on this device.':'Logging is active until you restart Redunar.';
 if(status==='off'||status==='stopped')return requested?'Restart Redunar to start logging.':'Logging is off.';
 return 'Logging status is unavailable.';
}
