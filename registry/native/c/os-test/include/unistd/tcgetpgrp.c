#include <unistd.h>
#ifdef tcgetpgrp
#undef tcgetpgrp
#endif
pid_t (*foo)(int) = tcgetpgrp;
int main(void) { return 0; }
