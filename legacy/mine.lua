_  = function(p) return p end
name = _('Mine')
description = 'Mine'
-- LEFT_MFCD = { x = 0, y = 1686 , width = 500 , height = 500  }
-- RIGHT_MFCD = { x = 500, y = 1686, width = 500, height = 500 }

mfd_outer_x = 0
mfd_outer_x_2 = 768
mfd_outer_x_3 = 1536
mfd_outer_y = 1686

mfd_inner_width = 638
mfd_inner_height = 648

mfd_outer_width = 768
mfd_outer_height = 778

mfd_inner_x = 65
mfd_inner_x_2 = 833
mfd_inner_y = 1751

function reconfigure_for_unit(unit_type)
  if unit_type == "AH-64D_BLK_II" then
    -- LEFT_MFCD_CPG = { x = 0, y = 1686, width = 768, height = 812 }
    LEFT_MFCD = { x = mfd_inner_x, y = mfd_inner_y, width = mfd_inner_width, height = mfd_inner_height }
    RIGHT_MFCD = { x = mfd_inner_x_2, y = mfd_inner_y, width = mfd_inner_width, height = mfd_inner_height }
    TEDAC = { x = mfd_outer_x_3, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    return
  end

  if unit_type == "AV8BNA" then
    LEFT_MFCD = { x = mfd_outer_x, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    RIGHT_MFCD = { x = mfd_outer_x_2, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    return
  end

  if unit_type == "FA-18C_hornet" then
    CENTER_MFCD = { x = mfd_outer_x, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    LEFT_MFCD = { x = mfd_outer_x_2, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    RIGHT_MFCD = { x = mfd_outer_x_3, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    return
  end

  if unit_type == "MiG-21Bis" then
    RADAR = { x = mfd_outer_x, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    MIG21_RADAR = { x = mfd_outer_x_2, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    return
  end

  if unit_type == "SA342M" or unit_type == "SA342L" or unit_type == "SA342Mistral" then
    NADIR_SCREEN = { x = mfd_outer_x, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    RWR = { x = mfd_outer_x_2, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    SA342_NADIR_SCREEN = { x = mfd_outer_x, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    SA342_RWR = { x = mfd_outer_x_2, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    return
  end

  if unit_type == "AJS37" then
    VIGGEN_TV = { x = mfd_outer_x, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    TV = { x = mfd_outer_x, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
    return
  end

  -- Default fallback
  LEFT_MFCD = { x = mfd_outer_x, y = mfd_outer_y , width = mfd_outer_width , height = mfd_outer_height }
  RIGHT_MFCD = { x = mfd_outer_x*2, y = mfd_outer_y, width = mfd_outer_width, height = mfd_outer_height }
end
Viewports = {
  Center = {
    x = 0,-- The HORIZONTAL starting location of the main display
    y = 0,-- The starting VERTICAL position of the main display
    width = 3440,-- The HORIZONTAL size of the main display
    height = 1686,-- The VERTICAL size of the main display
    aspect = 3440/1686,-- The aspect ratio of the main display
    dx = 0,
    dy = 0
  }
}
UIMainView = Viewports.Center
GU_MAIN_VIEWPORT = Viewports.Center