#include <unistd.h>
#ifdef posix_close
#undef posix_close
#endif
int (*foo)(int, int) = posix_close;
int main(void) { return 0; }
