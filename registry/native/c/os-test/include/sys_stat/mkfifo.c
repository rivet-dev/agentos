#include <sys/stat.h>
#ifdef mkfifo
#undef mkfifo
#endif
int (*foo)(const char *, mode_t) = mkfifo;
int main(void) { return 0; }
