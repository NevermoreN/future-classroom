const predicate = /[\u0000-\u001f\u007f-\u009f]/u;
for (const s of ["Synthetic TLS subject", "数学", "English", String.fromCharCode(0)]) console.log(JSON.stringify({input:s.length===1?"CONTROL_NUL":s,match:predicate.test(s)}));
