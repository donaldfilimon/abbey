/* Test-only POSIX metadata owner. Compile during fixture setup, never probe time. */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <signal.h>
#include <sys/wait.h>
#include <time.h>
#include <errno.h>
#include <limits.h>
#include <poll.h>
static volatile sig_atomic_t stopped;
static char root[4096], nonce[65], marker[257], python[4096], script[4096], path[4096];
static int metadata, hold, serial;
static void die(void) { perror("metadata fixture"); exit(2); }
static void cancel(int sig) { (void)sig; stopped = 1; }
static void field(FILE *f, char *out, size_t cap) {
    if (!fgets(out, (int)cap, f)) die();
    size_t n = strlen(out);
    if (!n || out[n-1] != '\n') { errno = EINVAL; die(); }
    out[n-1] = 0;
}
static void quoted(FILE *f, const char *s) {
    fputc('"',f);
    for (const unsigned char *p=(const unsigned char *)s; *p; p++) {
        if (*p=='"' || *p=='\\') fprintf(f,"\\%c",*p);
        else if (*p<32) fprintf(f,"\\u%04x",*p);
        else fputc(*p,f);
    }
    fputc('"',f);
}
static void event(const char *kind, const char *arg, pid_t leaf) {
    char dest[8192], temp[8208];
    snprintf(dest,sizeof dest,"%s/events/native-%ld-%d.json",root,(long)getpid(),serial++);
    snprintf(temp,sizeof temp,"%s.pending",dest);
    FILE *f=fopen(temp,"wx"); if (!f) die();
    fprintf(f,"{\"kind\":"); quoted(f,kind);
    fprintf(f,",\"nonce\":"); quoted(f,nonce);
    fprintf(f,",\"backend\":\"abi\",\"pid\":%ld,\"ppid\":%ld,\"pgid\":%ld,\"parent_pgid\":%ld,\"path\":",
            (long)getpid(),(long)getppid(),(long)getpgrp(),(long)getpgid(getppid()));
    quoted(f,path); fprintf(f,",\"argv\":["); quoted(f,arg);
    fprintf(f,"],\"action\":\"metadata\",\"leaf_pid\":");
    if(leaf>0) fprintf(f,"%ld",(long)leaf); else fprintf(f,"null");
    fprintf(f,"}\n"); if(fclose(f) || rename(temp,dest)) die();
}
static double now(void) { struct timespec t; if(clock_gettime(CLOCK_MONOTONIC,&t)) die(); return t.tv_sec+t.tv_nsec/1e9; }
static void pause_owned(void) {
    char release[8192]; snprintf(release,sizeof release,"%s/release",root);
    double end=now()+20;
    while(!stopped && access(release,F_OK) && now()<end) {
        struct timespec t={0,10000000}; nanosleep(&t,NULL);
    }
}
int main(int argc,char **argv) {
    const char *r=getenv("ABBEY_FIXTURE_ROOT");
    if(!r || strlen(r)>=sizeof root || r[0]!='/') { errno=EINVAL; die(); }
    strcpy(root,r);
    char control[8192], value[32]; snprintf(control,sizeof control,"%s/native-control",root);
    FILE *f=fopen(control,"r"); if(!f) die();
    field(f,nonce,sizeof nonce); field(f,value,sizeof value); metadata=!strcmp(value,"1");
    if(strcmp(value,"0") && strcmp(value,"1")) die();
    field(f,value,sizeof value); hold=!strcmp(value,"1");
    if(strcmp(value,"0") && strcmp(value,"1")) die();
    field(f,marker,sizeof marker); field(f,python,sizeof python); field(f,script,sizeof script);
    if(fgetc(f)!=EOF || strlen(nonce)!=32 || strspn(nonce,"0123456789abcdef")!=32 ||
       !*marker || strspn(marker,"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_")!=strlen(marker) ||
       python[0]!='/' || script[0]!='/') { errno=EINVAL; die(); }
    fclose(f);
    if(argc!=2 || (strcmp(argv[1],"--version") && strcmp(argv[1],"version"))) {
        char **forward=calloc((size_t)argc+3,sizeof(char*)); if(!forward) die();
        forward[0]=python; forward[1]="-S"; forward[2]=script;
        for(int i=1;i<argc;i++) forward[i+2]=argv[i];
        execv(python,forward); die();
    }
    if(!metadata) { puts("owned-fixture-version"); return 0; }
    if(!realpath(argv[0],path)) die();
    struct sigaction sa; memset(&sa,0,sizeof sa); sa.sa_handler=cancel; sigemptyset(&sa.sa_mask);
    if(sigaction(SIGTERM,&sa,NULL)) die();
    pid_t leaf=0;
    if(hold) {
        int ready[2]; if(pipe(ready)) die();
        leaf=fork(); if(leaf<0) die();
        if(!leaf) {
            close(ready[0]); event("leaf",argv[1],0);
            if(write(ready[1],"R",1)!=1) _exit(2);
            close(ready[1]);
            pause_owned(); _exit(stopped?130:0);
        }
        close(ready[1]); char byte; ssize_t n;
        struct pollfd pfd={ready[0],POLLIN,0};
        int ready_count=poll(&pfd,1,1000);
        n=ready_count>0 ? read(ready[0],&byte,1) : -1;
        close(ready[0]);
        if(n!=1) stopped=1;
    }
    if(!stopped) { event("call",argv[1],leaf); if(hold) pause_owned(); }
    if(leaf>0) {
        kill(leaf,SIGTERM); int status;
        while(waitpid(leaf,&status,0)<0) if(errno!=EINTR) die();
    }
    if(stopped) { event("terminated",argv[1],leaf); return 130; }
    puts(marker); return 0;
}
