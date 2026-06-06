#include <unistd.h>
#ifdef dup2
#undef dup2
#endif
int (*foo)(int, int) = dup2;
int main(void) { return 0; }
