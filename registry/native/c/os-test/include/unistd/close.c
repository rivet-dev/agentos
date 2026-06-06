#include <unistd.h>
#ifdef close
#undef close
#endif
int (*foo)(int) = close;
int main(void) { return 0; }
