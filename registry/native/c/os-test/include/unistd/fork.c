#include <unistd.h>
#ifdef fork
#undef fork
#endif
pid_t (*foo)(void) = fork;
int main(void) { return 0; }
