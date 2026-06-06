#include <sys/wait.h>
#ifdef wait
#undef wait
#endif
pid_t (*foo)(int *) = wait;
int main(void) { return 0; }
