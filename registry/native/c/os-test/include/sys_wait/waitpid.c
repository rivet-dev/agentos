#include <sys/wait.h>
#ifdef waitpid
#undef waitpid
#endif
pid_t (*foo)(pid_t, int *, int) = waitpid;
int main(void) { return 0; }
