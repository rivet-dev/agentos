#include <unistd.h>
#ifdef tcsetpgrp
#undef tcsetpgrp
#endif
int (*foo)(int, pid_t) = tcsetpgrp;
int main(void) { return 0; }
