#include <unistd.h>
#ifdef _Fork
#undef _Fork
#endif
pid_t (*foo)(void) = _Fork;
int main(void) { return 0; }
