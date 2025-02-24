#ifndef CPI_H
#define CPI_H

#include <stdbool.h> // bool
#include <stddef.h> // size_t
#include <shaderc/shaderc.h>

typedef enum {
	CPI_TYPE_NONE,
// CPU States
	CPI_TYPE_CONTEXT,
	CPI_TYPE_ID,
	CPI_TYPE_GROUP,
	CPI_TYPE_MUTEX,
	CPI_TYPE_BOX,
// CPU Operators
	CPI_TYPE_PROCESS,
	CPI_TYPE_THREAD,
	CPI_TYPE_FUNCTION,
// GPU States
	CPI_TYPE_WINDOW,
	CPI_TYPE_FENCE,
	CPI_TYPE_SAMPLER,
	CPI_TYPE_IMAGE,
	CPI_TYPE_TRANSFER,
	CPI_TYPE_BUFFER,
// GPU Operators
	CPI_TYPE_COMMAND,
	CPI_TYPE_RENDER_PASS,
	CPI_TYPE_COMPUTE_PASS,
	CPI_TYPE_COPY_PASS,
	CPI_TYPE_GRAPHICS_PIPELINE,
	CPI_TYPE_COMPUTE_PIPELINE,
	CPI_TYPE_SHADER
} CPI_TYPE;

typedef int CPI_ID_Handle;
typedef int CPI_Context_Handle;

// =============================================================================================================================
bool 					cpi_Initialize();
bool  					cpi_Destroy();

// =============================================================================================================================
CPI_Context_Handle 		CPI_Context_Create();
void 					CPI_Context_Delete(CPI_Context_Handle* context_handle);

// ID ==========================================================================================================================
CPI_TYPE  				cpi_ID_GetType(CPI_Context_Handle ctx, CPI_ID_Handle id);
bool 					cpi_ID_isValid(CPI_Context_Handle ctx, CPI_ID_Handle id);

// WINDOW ======================================================================================================================
CPI_ID_Handle  			cpi_Window_Create(CPI_Context_Handle ctx, unsigned int width, unsigned int height, const char* title);
void 					cpi_Window_Show(CPI_Context_Handle ctx, CPI_ID_Handle window);
CPI_ID_Handle  			cpi_Window_GetGroupID(CPI_Context_Handle ctx, CPI_ID_Handle window);
bool  					cpi_Window_Destroy(CPI_Context_Handle ctx, CPI_ID_Handle* p_window);

// BOX =========================================================================================================================
CPI_ID_Handle    		cpi_Box_Create(CPI_Context_Handle ctx, float x, float y, float width, float height, unsigned char red, unsigned char green, unsigned char blue, unsigned char alpha, float rotation_radians, float corner_radius_pixels, int sample_image, float sample_x, float sample_y, float sample_width, float sample_height);
CPI_ID_Handle    		cpi_Box_CreateEmpty(CPI_Context_Handle ctx);
void   					cpi_Box_Copy(CPI_Context_Handle ctx, CPI_ID_Handle src_box_id, CPI_ID_Handle dst_box_id);
void   					cpi_Box_SetPosition(CPI_Context_Handle ctx, CPI_ID_Handle box_id, float x, float y);
void   					cpi_Box_AddPosition(CPI_Context_Handle ctx, CPI_ID_Handle box_id, float d_x, float d_y);
void   					cpi_Box_SetSize(CPI_Context_Handle ctx, CPI_ID_Handle box_id, float width, float height);
void   					cpi_Box_AddSize(CPI_Context_Handle ctx, CPI_ID_Handle box_id, float d_width, float d_height);
void    				cpi_Box_SetColor(CPI_Context_Handle ctx, CPI_ID_Handle box_id, unsigned char red, unsigned char green, unsigned char blue, unsigned char alpha);
void   					cpi_Box_SetRotation(CPI_Context_Handle ctx, CPI_ID_Handle box_id, float rotation_radians);
void   					cpi_Box_AddRotation(CPI_Context_Handle ctx, CPI_ID_Handle box_id, float d_rotation_radians);
void   					cpi_Box_SetCornerRadius(CPI_Context_Handle ctx, CPI_ID_Handle box_id, float corner_radius_pixels);
void   					cpi_Box_SetImage(CPI_Context_Handle ctx, CPI_ID_Handle box_id, CPI_ID_Handle image_id);
void   					cpi_Box_SetImageSamplePos(CPI_Context_Handle ctx, CPI_ID_Handle box_id, float x, float y);
void   					cpi_Box_SetImageSampleSize(CPI_Context_Handle ctx, CPI_ID_Handle box_id, float width, float height);
void   					cpi_Box_Destroy(CPI_Context_Handle ctx, CPI_ID_Handle* p_box_id);

// IMAGE ========================================================================================================================
CPI_ID_Handle  			cpi_Image_Create(CPI_Context_Handle ctx, unsigned int width, unsigned int height, unsigned int format);
CPI_ID_Handle   		cpi_Image_GetWindowImage(CPI_Context_Handle ctx);
void 	  				cpi_Image_Write(CPI_Context_Handle ctx, CPI_ID_Handle image_id, void* p_src_data, unsigned int src_width, unsigned int src_height, unsigned int src_x, unsigned int src_y);
void   					cpi_Image_Read(CPI_Context_Handle ctx, CPI_ID_Handle image_id, void* p_dst_data, unsigned int dst_width, unsigned int dst_height, unsigned int dst_x, unsigned int dst_y);
void   					cpi_Image_Destroy(CPI_Context_Handle ctx, CPI_ID_Handle* p_image_id);  

// GROUP ========================================================================================================================
// group can contain box, group, function and render.
CPI_ID_Handle   		cpi_Group_Create(CPI_Context_Handle ctx, CPI_ID_Handle* p_element_ids, size_t elements_count);
void    				cpi_Group_Copy(CPI_Context_Handle ctx, CPI_ID_Handle src_group_id, CPI_ID_Handle dst_group_id);
void  					cpi_Group_AppendElement(CPI_Context_Handle ctx, CPI_ID_Handle group_id, CPI_ID_Handle new_element_id);
void 					cpi_Group_RemoveElement(CPI_Context_Handle ctx, CPI_ID_Handle group_id, CPI_ID_Handle element_id);
void   					cpi_Group_InsertElement(CPI_Context_Handle ctx, CPI_ID_Handle group_id, CPI_ID_Handle new_element_id, size_t index);
void   					cpi_Group_Move(CPI_Context_Handle ctx, CPI_ID_Handle group_id, float d_x, float d_y);
void   					cpi_Group_Rotate(CPI_Context_Handle ctx, CPI_ID_Handle group_id, float rotation_radians);
void  					cpi_Group_Destroy(CPI_Context_Handle ctx, CPI_ID_Handle* p_group_id);

// FUNCTION =====================================================================================================================
// function can be added to group. It is executed every iteration the function is "drawn" before all elements that actually gets drawn in the same render is drawn
CPI_ID_Handle   		cpi_Function_Create(CPI_Context_Handle ctx, void (*p_fn)(void*, size_t), void* p_data, size_t data_size);
void  					cpi_Function_Destroy(CPI_Context_Handle ctx, CPI_ID_Handle* p_function_id);

// COMMAND ======================================================================================================================
CPI_ID_Handle   		cpi_Command_Create(CPI_Context_Handle ctx);

// RENDER_PASS ======================================================================================================================
CPI_ID_Handle   		cpi_RenderPass_Create(CPI_Context_Handle ctx);

// COMPUTE_PASS ======================================================================================================================
CPI_ID_Handle   		cpi_ComputePass_Create(CPI_Context_Handle ctx);

// COPY_PASS ======================================================================================================================
CPI_ID_Handle   		cpi_CopyPass_Create(CPI_Context_Handle ctx);

// GRAPHICS_PIPELINE ======================================================================================================================
CPI_ID_Handle 			cpi_GraphicsPipeline_Create(CPI_Context_Handle ctx, CPI_ID_Handle vertex_shader_handle, CPI_ID_Handle fragment_shader_handle, bool enable_debug);

// COMPUTE_PIPELINE ======================================================================================================================
CPI_ID_Handle   		cpi_ComputePipeline_Create(CPI_Context_Handle ctx);

// SHADER ======================================================================================================================
void  					cpi_Shader_SpvFile_Create_FromGlslFile(CPI_Context_Handle ctx, const char* glsl_filename, const char* spv_filename, shaderc_shader_kind shader_kind);
CPI_ID_Handle 			cpi_Shader_CreateFromGlslFile(CPI_Context_Handle ctx, const char* glsl_file_path, const char* entrypoint, shaderc_shader_kind shader_kind, bool enable_debug);
void                    cpi_Shader_Destroy(CPI_Context_Handle ctx, CPI_ID_Handle* p_shader_is);


// FENCE ======================================================================================================================
CPI_ID_Handle   		cpi_Fence_Create(CPI_Context_Handle ctx);

#endif // CPI_H