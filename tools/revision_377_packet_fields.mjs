// Project original Java Packet values into Client.java operation semantics.
// Kept separate from Rust's projection: no Rust decoder or writer inverse runs.
export function operationFields(row, cases) {
    if (row.operation_fields) return row.operation_fields;
    const v = row.java_values;
    const tile = value => [(value >> 4) & 7, value & 7];
    const a = (kind, ...values) => [kind, ...values];
    let child;
    switch (row.opcode) {
    case 2: return a('SetAnim',v[0],v[1]);
    case 3: return a('CameraMoveTo',...v);
    case 5: return a('Logout');
    case 6: return a('NameDialog');
    case 10: return a('SetTab',v[0],v[1] === 65535 ? -1 : v[1]);
    case 13: return a('ResetAnims');
    case 18: return a('SetRotation',v[1],(v[0] << 16) + v[2] | 0);
    case 21: return a('SetObject',v[2],v[1],v[0]);
    case 26: return a('SynthSound',...v);
    case 29: return a('Close');
    case 40: return a('Zone',[v[1],v[0]],true,[]);
    case 41: child=a('SoundArea',...tile(v[0]),v[1],v[2] >> 4 & 15,v[2] & 7); break;
    case 49: return a('UpdateStat',...v);
    case 50: return a('OpenOverlay',v[0]);
    case 58: return a('CountDialog');
    case 59: child=a('MapAnim',...tile(v[0]),...v.slice(1)); break;
    case 61: return a('UnsetMapFlag');
    case 63: return a('MessageGame',v[0]);
    case 67: return a('CameraShake',...v);
    case 75: return a('Zone',v,false,[]);
    case 76: return a('LastLogin',...v);
    case 78: return a('Friend',...v);
    case 82: return a('SetHide',v[1],v[0] === 1);
    case 88: child=a('LocDel',...tile(v[0]),v[1] >> 2,v[1] & 3); break;
    case 106: child=a('ObjectReveal',...tile(v[0]),...v.slice(1)); break;
    case 107: child=a('ObjAdd',...tile(v[1]),v[0],v[2]); break;
    case 109: return a('OpenChat',v[0]);
    case 113: return a('VarpSync');
    case 115: return a('Varp',v[1],v[0]);
    case 121: child=a('ObjCount',...tile(v[0]),v[1] & 32767,v[2],v[3]); break;
    case 125: return a('RunEnergy',v[0]);
    case 126: return a('UpdatePid',...v);
    case 128: return a('OpenMainSide',...v);
    case 134: {
        const entries=[];
        for (let i=1;i<v.length;) { const slot=v[i++], object=v[i++], count=v[i++]; entries.push([slot,object,count===255 ? v[i++] : count]); }
        return a('InventoryPartial',v[0],entries);
    }
    case 135: return a('PrivateMessage',...v);
    case 142: child=a('LocAnim',...tile(v[2]),v[1] >> 2 === 11 ? 10 : v[1] >> 2,v[1] & 3,v[0]); break;
    case 148: return a('CameraReset');
    case 152: child=a('LocAddChange',...tile(v[2]),v[0] >> 2,v[0] & 3,v[1]); break;
    case 156: return a('MinimapToggle',v[0]);
    case 157: return a('PlayerOption',...v);
    case 158: return a('TutOpen',v[0]);
    case 159: return a('OpenMain',v[0]);
    case 162: return a('SetNpcHead',v[1],v[0]);
    case 166: return a('SetPosition',v[2],v[1],v[0]);
    case 167: return a('CameraLookAt',...v);
    case 174: return a('RunWeight',v[0]);
    case 181: child=a('MapProjAnim',...tile(v[0]),v[1],v[2],v[3],v[4]===65535 ? null : v[4],v[5]*4,v[6]*4,...v.slice(7)); break;
    case 182: return a('Varp',...v);
    case 183: return a('Zone',v.slice(0,2),false,row.children.map(id => operationFields(cases.find(c=>c.opcode===id),cases)[3][0]));
    case 186: return a('SetAngle',v[1],v[0],v[3],v[2]);
    case 190: return a('RebootTimer',v[0]);
    case 199: return v[0]===1 ? a('HintNpc',v[1],v[2]) : v[0]===10 ? a('HintPlayer',v[1],v[2]) : v[0]>=2 && v[0]<=6 ? a('HintTile',...v) : a('HintOther',...v);
    case 200: return a('SetScrollPos',...v);
    case 201: return a('ChatFilter',...v);
    case 203: child=a('LocMerge',...tile(v[3]),v[1] >> 2,v[1] & 3,v[0],Math.min(v[2],v[4]),Math.max(v[2],v[4]),Math.min(v[7],v[8]),Math.max(v[7],v[8]),v[6],v[9],v[5]); break;
    case 206: {
        const entries=[];
        for (let i=2;i<v.length;) { const object=v[i++],count=v[i++]; entries.push([object,count===255 ? v[i++] : count]); }
        if (entries.length!==v[1]) throw new Error('full inventory projection count');
        return a('InventoryFull',v[0],entries);
    }
    case 208: child=a('ObjDel',...tile(v[1]),v[0] & 32767); break;
    case 216: return a('SetModel',v[1],v[0]);
    case 218: return a('SetColour',...v);
    case 219: return a('InventoryStopTransmit',v[0]);
    case 222: return a('RebuildNormal',v[1],v[0]);
    case 220: return a('MidiSong',v[0]===65535 ? -1 : v[0]);
    case 226: return a('IgnoreList',v);
    case 232: return a('SetText',...v);
    case 233: return a('Multiway',v[0]);
    case 238: return a('TutFlash',v[0]);
    case 246: return a('OpenSide',v[0]);
    case 249: return a('MidiJingle',...v);
    case 251: return a('FriendListLoaded',v[0]);
    case 252: return a('SetTabActive',v[0]);
    case 253: return a('OpenFull',...v);
    case 255: return a('SetPlayerHead',v[0]);
    default: throw new Error(`missing semantic projection ${row.opcode}`);
    }
    return a('Zone',null,false,[child]);
}
