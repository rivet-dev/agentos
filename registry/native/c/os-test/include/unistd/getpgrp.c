#include <unistd.h>
#ifdef getpgrp
#undef getpgrp
#endif
pid_t (*foo)(void) = getpgrp;
int main(void) { return 0; }
