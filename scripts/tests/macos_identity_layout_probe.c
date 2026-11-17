/* Target-SDK layout evidence only. No runtime state or process inspection. */
#if !defined(__APPLE__) || !defined(__MACH__)
#error "This probe requires the actual macOS SDK"
#endif

#include <stddef.h>
#include <stdio.h>
#include <sys/proc_info.h>
#include <sys/stat.h>

#define FIELD(type, field) \
    printf(",\"" #field "\":%zu", offsetof(struct type, field))

int main(void) {
    printf("{\"pointer_size\":%zu,\"proc_regioninfo\":{\"size\":%zu",
           sizeof(void *), sizeof(struct proc_regioninfo));
    FIELD(proc_regioninfo, pri_offset);
    FIELD(proc_regioninfo, pri_address);
    FIELD(proc_regioninfo, pri_size);
    printf("},\"proc_regionwithpathinfo\":{\"size\":%zu",
           sizeof(struct proc_regionwithpathinfo));
    FIELD(proc_regionwithpathinfo, prp_prinfo);
    FIELD(proc_regionwithpathinfo, prp_vip);
    printf("},\"vnode_info_path\":{\"size\":%zu",
           sizeof(struct vnode_info_path));
    FIELD(vnode_info_path, vip_vi);
    FIELD(vnode_info_path, vip_path);
    printf("},\"vnode_info\":{\"size\":%zu", sizeof(struct vnode_info));
    FIELD(vnode_info, vi_stat);
    FIELD(vnode_info, vi_type);
    printf("},\"vinfo_stat\":{\"size\":%zu", sizeof(struct vinfo_stat));
    FIELD(vinfo_stat, vst_dev);
    FIELD(vinfo_stat, vst_ino);
    FIELD(vinfo_stat, vst_size);
    FIELD(vinfo_stat, vst_mtime);
    FIELD(vinfo_stat, vst_mtimensec);
    printf("},\"proc_bsdinfo\":{\"size\":%zu", sizeof(struct proc_bsdinfo));
    FIELD(proc_bsdinfo, pbi_pid);
    FIELD(proc_bsdinfo, pbi_ppid);
    FIELD(proc_bsdinfo, pbi_uid);
    FIELD(proc_bsdinfo, pbi_pgid);
    FIELD(proc_bsdinfo, pbi_start_tvsec);
    FIELD(proc_bsdinfo, pbi_start_tvusec);
    printf("},\"stat\":{\"device_size\":%zu,\"device_signed\":%s}}\n",
           sizeof(((struct stat *)0)->st_dev), ((dev_t)-1 < 0) ? "true" : "false");
    return ferror(stdout) ? 1 : 0;
}
