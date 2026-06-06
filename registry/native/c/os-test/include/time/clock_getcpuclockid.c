/*[CPT]*/
#include <time.h>
#ifdef clock_getcpuclockid
#undef clock_getcpuclockid
#endif
int (*foo)(pid_t, clockid_t *) = clock_getcpuclockid;
int main(void) { return 0; }
