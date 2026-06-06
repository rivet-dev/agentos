#include <wchar.h>
#ifdef wscanf
#undef wscanf
#endif
int (*foo)(const wchar_t *restrict, ...) = wscanf;
int main(void) { return 0; }
